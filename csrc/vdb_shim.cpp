// OpenVDB voxel remesh exactly as Blender 4.2's BKE_mesh_remesh_voxel (blenkernel/intern/mesh_remesh_voxel.cc): meshToLevelSet(halfWidth 1) + volumeToMesh.
#include <openvdb/openvdb.h>
#include <openvdb/tools/MeshToVolume.h>
#include <openvdb/tools/VolumeToMesh.h>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <vector>

extern "C" {

static std::once_flag g_init;

// Returns 0 on success. Output arrays are malloc'ed (free with vdb_free): points (3 floats each), quads (4 uints), triangles (3 uints).
int vdb_remesh_voxel(const float *pts, int npts, const unsigned *tris, int ntri, float voxel_size, float adaptivity, float isovalue,
                     float **out_pts, int *out_npts, unsigned **out_quads, int *out_nquads, unsigned **out_tris, int *out_ntris)
{
  std::call_once(g_init, []() { openvdb::initialize(); });
  std::vector<openvdb::Vec3s> points(npts);
  for (int i = 0; i < npts; i++) {
    points[i] = openvdb::Vec3s(pts[3 * i], pts[3 * i + 1], pts[3 * i + 2]);
  }
  std::vector<openvdb::Vec3I> triangles(ntri);
  for (int i = 0; i < ntri; i++) {
    triangles[i] = openvdb::Vec3I(tris[3 * i], tris[3 * i + 1], tris[3 * i + 2]);
  }
  openvdb::math::Transform::Ptr transform = openvdb::math::Transform::createLinearTransform(voxel_size);
  openvdb::FloatGrid::Ptr grid = openvdb::tools::meshToLevelSet<openvdb::FloatGrid>(*transform, points, triangles, 1.0f);

  std::vector<openvdb::Vec3s> vertices;
  std::vector<openvdb::Vec4I> quads;
  std::vector<openvdb::Vec3I> tri_out;
  openvdb::tools::volumeToMesh<openvdb::FloatGrid>(*grid, vertices, tri_out, quads, isovalue, adaptivity, false);

  *out_npts = (int)vertices.size();
  *out_nquads = (int)quads.size();
  *out_ntris = (int)tri_out.size();
  *out_pts = (float *)malloc(sizeof(float) * 3 * (vertices.size() + 1));
  *out_quads = (unsigned *)malloc(sizeof(unsigned) * 4 * (quads.size() + 1));
  *out_tris = (unsigned *)malloc(sizeof(unsigned) * 3 * (tri_out.size() + 1));
  for (size_t i = 0; i < vertices.size(); i++) {
    (*out_pts)[3 * i] = vertices[i].x();
    (*out_pts)[3 * i + 1] = vertices[i].y();
    (*out_pts)[3 * i + 2] = vertices[i].z();
  }
  for (size_t i = 0; i < quads.size(); i++) {
    for (int k = 0; k < 4; k++) {
      (*out_quads)[4 * i + k] = quads[i][k];
    }
  }
  for (size_t i = 0; i < tri_out.size(); i++) {
    for (int k = 0; k < 3; k++) {
      (*out_tris)[3 * i + k] = tri_out[i][k];
    }
  }
  return 0;
}

void vdb_free(void *p) { free(p); }
}
