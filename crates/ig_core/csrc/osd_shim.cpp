// Thin C ABI over OpenSubdiv 3.6.0 (the version Blender 4.2 bundles), driving it the way Blender 4.2's CPU subsurf does
// (intern/opensubdiv evaluator_impl.cc + blenkernel subdiv_converter_mesh.cc): uniform refinement to `level`, vertex stencils with the
// patch table's local point stencils appended, Gregory-basis end caps, CPU evaluator, one limit point per (ptex face, u, v).
// Own code written against the public OpenSubdiv API (Blender's sources are GPL; OpenSubdiv is Apache 2.0).
#include <opensubdiv/far/patchMap.h>
#include <opensubdiv/far/patchTable.h>
#include <opensubdiv/far/patchTableFactory.h>
#include <opensubdiv/far/stencilTable.h>
#include <opensubdiv/far/stencilTableFactory.h>
#include <opensubdiv/far/topologyRefiner.h>
#include <opensubdiv/far/topologyRefinerFactory.h>
#include <opensubdiv/osd/cpuEvaluator.h>
#include <opensubdiv/osd/cpuPatchTable.h>
#include <opensubdiv/osd/cpuVertexBuffer.h>
#include <opensubdiv/sdc/options.h>

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <vector>

using namespace OpenSubdiv;

static int ev(const char *n, int d) { const char *s = getenv(n); return s ? atoi(s) : d; }

namespace {

struct MeshData {
  int num_verts;
  int num_faces;
  const int *face_sizes;
  const int *face_verts;
  bool simple;
  const int *fvar = nullptr;  // optional face-varying (UV) value index per face corner; null = no UV layer
  int num_fvar = 0;
};

}  // namespace

namespace OpenSubdiv {
namespace OPENSUBDIV_VERSION {
namespace Far {

template<> inline bool TopologyRefinerFactory<MeshData>::resizeComponentTopology(TopologyRefiner &refiner, const MeshData &m)
{
  setNumBaseVertices(refiner, m.num_verts);
  setNumBaseFaces(refiner, m.num_faces);
  for (int f = 0; f < m.num_faces; ++f) {
    setNumBaseFaceVertices(refiner, f, m.face_sizes[f]);
  }
  return true;
}

template<> inline bool TopologyRefinerFactory<MeshData>::assignComponentTopology(TopologyRefiner &refiner, const MeshData &m)
{
  int off = 0;
  for (int f = 0; f < m.num_faces; ++f) {
    IndexArray dst = getBaseFaceVertices(refiner, f);
    for (int i = 0; i < m.face_sizes[f]; ++i) {
      dst[i] = m.face_verts[off + i];
    }
    off += m.face_sizes[f];
  }
  return true;
}

template<> inline bool TopologyRefinerFactory<MeshData>::assignComponentTags(TopologyRefiner &refiner, const MeshData &m)
{
  // BUGGY_SIMPLE_SCHEME_WORKAROUND: simple subdivision = Catmull-Clark with every edge (sharpness 10) and vertex infinitely sharp.
  // Normal case: no creases; vertices where exactly two edges meet add min(edge sharpness) = 0.
  if (m.simple) {
    for (int e = 0; e < refiner.GetLevel(0).GetNumEdges(); ++e) {
      setBaseEdgeSharpness(refiner, e, 10.0f);
    }
    for (int v = 0; v < m.num_verts; ++v) {
      setBaseVertexSharpness(refiner, v, Sdc::Crease::SHARPNESS_INFINITE);
    }
  }
  return true;
}

template<> inline bool TopologyRefinerFactory<MeshData>::assignFaceVaryingTopology(TopologyRefiner &refiner, const MeshData &m)
{
  if (m.fvar) {
    int ch = createBaseFVarChannel(refiner, m.num_fvar);
    int off = 0;
    for (int f = 0; f < m.num_faces; ++f) {
      IndexArray dst = getBaseFaceFVarValues(refiner, f, ch);
      for (int i = 0; i < m.face_sizes[f]; ++i) {
        dst[i] = m.fvar[off + i];
      }
      off += m.face_sizes[f];
    }
  }
  return true;
}

template<> inline void TopologyRefinerFactory<MeshData>::reportInvalidTopology(TopologyError, const char *msg, const MeshData &)
{
  std::printf("OpenSubdiv Error: %s\n", msg);
}

}  // namespace Far
}  // namespace OPENSUBDIV_VERSION
}  // namespace OpenSubdiv

namespace {

template<typename T> struct RawBuf {
  T *data;
  explicit RawBuf(T *d) : data(d) {}
  T *BindCpuBuffer() { return data; }
  int BindVBO() { return 0; }
};

struct PatchCoordBuf {
  const Osd::PatchCoord *data;
  int n;
  PatchCoordBuf(const Osd::PatchCoord *d, int n_) : data(d), n(n_) {}
  const Osd::PatchCoord *BindCpuBuffer() { return data; }
  int BindVBO() { return 0; }
  int GetNumVertices() { return n; }
};

struct Evaluator {
  Far::TopologyRefiner *refiner = nullptr;
  Far::PatchTable *patch_table = nullptr;
  Far::PatchMap *patch_map = nullptr;
  Far::StencilTable const *stencils = nullptr;
  Osd::CpuPatchTable *cpu_patch_table = nullptr;
  Osd::CpuVertexBuffer *src = nullptr;
  Osd::BufferDescriptor src_desc;
  int num_coarse = 0;
};

}  // namespace

extern "C" {

// positions: float[3 * num_verts]; face_sizes: int[num_faces]; face_verts: concatenated
// fvar: optional face-varying (UV) value id per face corner (loops sharing a vertex AND a UV value share an id), num_fvar = number of ids; Blender
// builds this channel for every mesh with a UV map, and it changes the adaptive isolation (hence the last float bits). null = no UV map.
void *osd_create_uv(int num_verts, const float *positions, int num_faces, const int *face_sizes, const int *face_verts, int level, int adaptive, int simple,
                    int boundary_corner, const int *fvar, int num_fvar)
{
  adaptive = ev("OSD_ADAPT", adaptive);
  level = ev("OSD_LEVEL", level);
  MeshData md{num_verts, num_faces, face_sizes, face_verts, simple != 0, fvar, num_fvar};
  std::vector<int> fv;
  if (const char *fp = getenv("OSD_FVAR")) {  // experiment: file of per-corner uv value ids
    FILE *fh = fopen(fp, "r");
    int x;
    while (fh && fscanf(fh, "%d", &x) == 1) fv.push_back(x);
    if (fh) fclose(fh);
    md.fvar = fv.data();
    md.num_fvar = 1 + *std::max_element(fv.begin(), fv.end());
  }
  Sdc::Options sdc;
  sdc.SetVtxBoundaryInterpolation(boundary_corner ? Sdc::Options::VTX_BOUNDARY_EDGE_AND_CORNER : Sdc::Options::VTX_BOUNDARY_EDGE_ONLY);
  sdc.SetCreasingMethod(Sdc::Options::CREASE_UNIFORM);
  sdc.SetFVarLinearInterpolation((Sdc::Options::FVarLinearInterpolation)ev("OSD_FVLIN", md.fvar ? Sdc::Options::FVAR_LINEAR_BOUNDARIES : Sdc::Options::FVAR_LINEAR_ALL));
  Far::TopologyRefinerFactory<MeshData>::Options opts(Sdc::SCHEME_CATMARK, sdc);
  opts.validateFullTopology = false;
  Far::TopologyRefiner *refiner = Far::TopologyRefinerFactory<MeshData>::Create(md, opts);
  if (!refiner) {
    return nullptr;
  }
  if (adaptive) {
    Far::TopologyRefiner::AdaptiveOptions ao(getenv("OSD_ISO") ? atoi(getenv("OSD_ISO")) : level);
    ao.considerFVarChannels = ev("OSD_CONSIDER", md.fvar != nullptr);
    ao.useInfSharpPatch = ev("OSD_INFSHARP", 1);
    ao.useSingleCreasePatch = ev("OSD_SINGLECREASE", 0);
    refiner->RefineAdaptive(ao);
  }
  else {
    Far::TopologyRefiner::UniformOptions uo(level);
    refiner->RefineUniform(uo);
  }

  Far::StencilTableFactory::Options so;
  so.generateOffsets = true;
  so.generateIntermediateLevels = ev("OSD_INTER", adaptive != 0);
  Far::StencilTable const *stencils = Far::StencilTableFactory::Create(*refiner, so);

  Far::PatchTableFactory::Options po(level);
  po.SetEndCapType((Far::PatchTableFactory::Options::EndCapType)ev("OSD_ENDCAP", Far::PatchTableFactory::Options::ENDCAP_GREGORY_BASIS));
  po.useInfSharpPatch = ev("OSD_INFSHARP", 1);
  po.useSingleCreasePatch = ev("OSD_SINGLECREASE", 0);
  po.generateLegacySharpCornerPatches = ev("OSD_LEGACYCORNER", 1);
  po.triangulateQuads = ev("OSD_TRI", 0);
  po.shareEndCapPatchPoints = ev("OSD_SHARE", 1);
  po.generateFVarTables = false;
  po.generateFVarLegacyLinearPatches = false;
  Far::PatchTable *pt = Far::PatchTableFactory::Create(*refiner, po);

  Far::StencilTable const *local = pt->GetLocalPointStencilTable();
  if (local) {
    Far::StencilTable const *t = Far::StencilTableFactory::AppendLocalPointStencilTable(*refiner, stencils, local);
    delete stencils;
    stencils = t;
  }
  Evaluator *ev = new Evaluator();
  ev->refiner = refiner;
  ev->patch_table = pt;
  ev->patch_map = new Far::PatchMap(*pt);
  ev->stencils = stencils;
  ev->cpu_patch_table = Osd::CpuPatchTable::Create(pt);
  ev->num_coarse = stencils->GetNumControlVertices();
  int total = ev->num_coarse + stencils->GetNumStencils();
  ev->src = Osd::CpuVertexBuffer::Create(3, total);
  ev->src_desc = Osd::BufferDescriptor(0, 3, 3);
  ev->src->UpdateData(positions, 0, num_verts);
  // refine: stencils of the final level (+ local points) into the tail of the buffer
  Osd::BufferDescriptor dst_desc = ev->src_desc;
  dst_desc.offset += ev->num_coarse * ev->src_desc.stride;
  Osd::CpuEvaluator::EvalStencils(ev->src, ev->src_desc, ev->src, dst_desc, stencils);
  return ev;
}

void *osd_create(int num_verts, const float *positions, int num_faces, const int *face_sizes, const int *face_verts, int level, int adaptive, int simple,
                 int boundary_corner)
{
  return osd_create_uv(num_verts, positions, num_faces, face_sizes, face_verts, level, adaptive, simple, boundary_corner, nullptr, 0);
}

void osd_eval(void *h, int n, const int *ptex, const float *u, const float *v, float *out)
{
  Evaluator *ev = static_cast<Evaluator *>(h);
  std::vector<Osd::PatchCoord> pcs(n);
  for (int i = 0; i < n; ++i) {
    const Far::PatchTable::PatchHandle *handle = ev->patch_map->FindPatch(ptex[i], u[i], v[i]);
    pcs[i] = Osd::PatchCoord(*handle, u[i], v[i]);
  }
  // one coordinate at a time, as BKE_subdiv does (evaluateLimit)
  for (int i = 0; i < n; ++i) {
    RawBuf<float> P(out + 3 * i);
    Osd::BufferDescriptor P_desc(0, 3, 3);
    PatchCoordBuf pcb(&pcs[i], 1);
    float dud[3], dvd[3];
    RawBuf<float> DU(dud), DV(dvd);
    if (getenv("OSD_NODERIV")) Osd::CpuEvaluator::EvalPatches(ev->src, ev->src_desc, &P, P_desc, 1, &pcb, ev->cpu_patch_table);
    else Osd::CpuEvaluator::EvalPatches(ev->src, ev->src_desc, &P, P_desc, &DU, P_desc, &DV, P_desc, 1, &pcb, ev->cpu_patch_table);
  }
}

int osd_num_ptex(void *h)
{
  Evaluator *ev = static_cast<Evaluator *>(h);
  return ev->refiner->GetLevel(0).GetNumFaces();  // placeholder: ptex count is computed on the Rust side
}

void osd_free(void *h)
{
  Evaluator *ev = static_cast<Evaluator *>(h);
  if (!ev) {
    return;
  }
  delete ev->src;
  delete ev->cpu_patch_table;
  delete ev->patch_map;
  delete ev->patch_table;
  delete ev->stencils;
  delete ev->refiner;
  delete ev;
}

}  // extern "C"
