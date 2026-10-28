// Cloth jobs (bed linen, pillows) on library soft bodies: Jolt Physics (MIT) and Bullet (zlib). One C entry point per backend; both
// take the job in the units of crates/ig_kit/src/cloth.rs (Blender CLOTH settings, simulation time in frames) and return world
// positions. Colliders are static triangle meshes. Pressure is converted from Blender's constant uniform pressure to the libraries'
// ideal-gas form (coefficient / volume) so that it is equal at the rest volume.
// Tuning (calibration against Blender): environment variables CLJ_* (Jolt) and CLB_* (Bullet), see the env() calls.
#include <cmath>
#include <cstdio>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <thread>
#include <vector>

// ---------------------------------------------------------------- shared
extern "C" {
struct ClothJobC {
  int nv;
  const double *pos;     // nv * 3
  int ntri;
  const int *tri;        // ntri * 3 (triangulated faces)
  const double *pin;     // nv weights (>= 0.999 fixed) or null
  double mass;           // per vertex
  double tension, compression, shear, bending;
  double tension_damp, shear_damp, air_damping;
  double gravity;        // effector weight
  double pressure;       // Blender uniform pressure (0 = none), already scaled by the caller's calibration
  double distance_min;
  int end_frame;
  int self_collision;
  double avg_len;        // mean structural edge length (stiffness scale, as cloth.rs)
  int ncol;
  const int *col_nv;     // per collider vertex count
  const double *const *col_pos;
  const int *col_ntri;
  const int *const *col_tri;
};
}

static double env(const char *k, double d) {
  const char *s = getenv(k);
  return s ? atof(s) : d;
}

// one frame of Blender cloth time in seconds: the implicit solver's calibrated gravity is 0.00981 m/frame^2 = 9.81 m/s^2 * T^2
static double frame_seconds() { return std::sqrt(0.001); }

static double rest_volume(const ClothJobC *j) {
  double v = 0;
  for (int t = 0; t < j->ntri; ++t) {
    const double *a = j->pos + 3 * j->tri[3 * t], *b = j->pos + 3 * j->tri[3 * t + 1], *c = j->pos + 3 * j->tri[3 * t + 2];
    v += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]);
  }
  return std::fabs(v) / 6.0;
}

// ---------------------------------------------------------------- Jolt
#include <Jolt/Jolt.h>
#include <Jolt/Core/Factory.h>
#include <Jolt/Core/JobSystemThreadPool.h>
#include <Jolt/Core/TempAllocator.h>
#include <Jolt/Physics/Body/BodyCreationSettings.h>
#include <Jolt/Physics/Collision/Shape/MeshShape.h>
#include <Jolt/Physics/PhysicsSystem.h>
#include <Jolt/Physics/SoftBody/SoftBodyCreationSettings.h>
#include <Jolt/Physics/SoftBody/SoftBodyMotionProperties.h>
#include <Jolt/Physics/SoftBody/SoftBodySharedSettings.h>
#include <Jolt/RegisterTypes.h>

namespace {
using namespace JPH;
namespace Lay {
constexpr ObjectLayer STATIC = 0, MOVING = 1, NUM = 2;
}
class OLPF : public ObjectLayerPairFilter {
 public:
  bool ShouldCollide(ObjectLayer a, ObjectLayer b) const override { return a == Lay::MOVING || b == Lay::MOVING; }
};
class BPLI final : public BroadPhaseLayerInterface {
 public:
  uint GetNumBroadPhaseLayers() const override { return 2; }
  BroadPhaseLayer GetBroadPhaseLayer(ObjectLayer l) const override { return BroadPhaseLayer((BroadPhaseLayer::Type)l); }
#if defined(JPH_EXTERNAL_PROFILE) || defined(JPH_PROFILE_ENABLED)
  const char *GetBroadPhaseLayerName(BroadPhaseLayer) const override { return "L"; }
#endif
};
class OVBF : public ObjectVsBroadPhaseLayerFilter {
 public:
  bool ShouldCollide(ObjectLayer a, BroadPhaseLayer b) const override { return a == Lay::MOVING || (BroadPhaseLayer::Type)b == Lay::MOVING; }
};
std::once_flag jolt_once;
}  // namespace

extern "C" int cloth_jolt(const ClothJobC *j, double *out) {
  std::call_once(jolt_once, [] {
    RegisterDefaultAllocator();
    Factory::sInstance = new Factory();
    RegisterTypes();
  });
  const int threads = (int)env("CLJ_THREADS", 1);
  TempAllocatorImpl temp(64 * 1024 * 1024);
  JobSystemThreadPool jobs(cMaxPhysicsJobs, cMaxPhysicsBarriers, threads > 1 ? threads - 1 : 0);
  BPLI bpli;
  OVBF ovbf;
  OLPF olpf;
  PhysicsSystem ps;
  ps.Init(1024, 0, 1024, 1024, bpli, ovbf, olpf);
  const double T = frame_seconds();
  ps.SetGravity(Vec3(0, 0, -9.81f));
  BodyInterface &bi = ps.GetBodyInterface();
  for (int c = 0; c < j->ncol; ++c) {
    TriangleList tl;
    const double *p = j->col_pos[c];
    for (int t = 0; t < j->col_ntri[c]; ++t) {
      const int *ix = j->col_tri[c] + 3 * t;
      auto v = [&](int k) { return Float3((float)p[3 * ix[k]], (float)p[3 * ix[k] + 1], (float)p[3 * ix[k] + 2]); };
      tl.push_back(Triangle(v(0), v(1), v(2)));
    }
    MeshShapeSettings ms(tl);
    auto sr = ms.Create();
    if (sr.HasError()) continue;
    BodyCreationSettings bcs(sr.Get(), RVec3::sZero(), Quat::sIdentity(), EMotionType::Static, Lay::STATIC);
    bcs.mFriction = (float)env("CLJ_COLFRIC", 1.0);
    bi.CreateAndAddBody(bcs, EActivation::DontActivate);
  }
  // spring stiffness as cloth.rs (force per m, frames as time) -> SI -> XPBD compliance = 1 / k
  const double ak = env("CLJ_K", 1.0) / std::max(j->avg_len, 1e-9);
  const double si = 1.0 / (T * T);
  auto compliance = [&](double k) { return k > 0 ? (float)(1.0 / (k * ak * si)) : FLT_MAX; };
  Ref<SoftBodySharedSettings> sh = new SoftBodySharedSettings;
  for (int i = 0; i < j->nv; ++i) {
    bool pinned = j->pin && j->pin[i] >= 0.999;
    sh->mVertices.push_back(SoftBodySharedSettings::Vertex(Float3((float)j->pos[3 * i], (float)j->pos[3 * i + 1], (float)j->pos[3 * i + 2]), Float3(0, 0, 0), pinned ? 0.0f : (float)(1.0 / j->mass)));
  }
  for (int t = 0; t < j->ntri; ++t) {
    SoftBodySharedSettings::Face f(j->tri[3 * t], j->tri[3 * t + 1], j->tri[3 * t + 2]);
    if (!f.IsDegenerate()) sh->AddFace(f);
  }
  SoftBodySharedSettings::VertexAttributes va(compliance(j->tension), compliance(j->shear * env("CLJ_SHEAR", 1.0)), env("CLJ_BEND", 1.0) > 0 ? compliance(j->bending * env("CLJ_BEND", 1.0)) : FLT_MAX);
  sh->CreateConstraints(&va, 1, env("CLJ_DIHEDRAL", 1.0) > 0.5 ? SoftBodySharedSettings::EBendType::Dihedral : SoftBodySharedSettings::EBendType::Distance);
  sh->Optimize();
  SoftBodyCreationSettings sbs(sh, RVec3::sZero(), Quat::sIdentity(), Lay::MOVING);
  sbs.mUpdatePosition = false;
  sbs.mMakeRotationIdentity = false;
  sbs.mAllowSleeping = false;
  sbs.mNumIterations = (uint32)env("CLJ_ITERS", 8);
  sbs.mLinearDamping = (float)(env("CLJ_DAMP", 1.0) * j->air_damping * 0.1);
  sbs.mGravityFactor = (float)j->gravity;
  sbs.mFriction = (float)env("CLJ_FRIC", 0.8);
  sbs.mVertexRadius = (float)(env("CLJ_RADIUS", 1.0) * j->distance_min);
  sbs.mFacesDoubleSided = true;
  sbs.mPressure = j->pressure != 0 ? (float)(j->pressure * si * env("CLJ_PRESS", 1.0) * rest_volume(j)) : 0.0f;
  BodyID id = bi.CreateAndAddSoftBody(sbs, EActivation::Activate);
  const int sub = (int)env("CLJ_SUB", 4);
  const int steps = std::max(0, j->end_frame - 1) * sub;
  const float dt = (float)(T / sub);
  // constant pressure (Blender's uniform pressure): Jolt's pressure is n R T / V, so set n R T = p * V every step
  const double p_si = j->pressure * si * env("CLJ_PRESS", 1.0);
  const bool constant_p = j->pressure != 0 && env("CLJ_IDEALGAS", 0.0) < 0.5;
  for (int s = 0; s < steps; ++s) {
    if (constant_p) {
      BodyLockWrite lock(ps.GetBodyLockInterface(), id);
      auto *mp = static_cast<SoftBodyMotionProperties *>(lock.GetBody().GetMotionProperties());
      mp->SetPressure((float)(p_si * std::max(1e-6f, std::fabs(mp->GetVolume()))));
    }
    ps.Update(dt, 1, &temp, &jobs);
  }
  {
    BodyLockRead lock(ps.GetBodyLockInterface(), id);
    const Body &b = lock.GetBody();
    const auto *mp = static_cast<const SoftBodyMotionProperties *>(b.GetMotionProperties());
    RMat44 m = b.GetCenterOfMassTransform();
    const auto &vs = mp->GetVertices();
    for (int i = 0; i < j->nv; ++i) {
      RVec3 w = m * vs[i].mPosition;
      out[3 * i] = w.GetX(), out[3 * i + 1] = w.GetY(), out[3 * i + 2] = w.GetZ();
    }
  }
  bi.RemoveBody(id);
  bi.DestroyBody(id);
  return 0;
}

// ---------------------------------------------------------------- Bullet
#include <BulletCollision/CollisionShapes/btBvhTriangleMeshShape.h>
#include <BulletCollision/CollisionShapes/btTriangleIndexVertexArray.h>
#include <BulletSoftBody/btDefaultSoftBodySolver.h>
#include <BulletSoftBody/btSoftBodyHelpers.h>
#include <BulletSoftBody/btSoftBodyRigidBodyCollisionConfiguration.h>
#include <BulletSoftBody/btSoftRigidDynamicsWorld.h>
#include <btBulletDynamicsCommon.h>

extern "C" int cloth_bullet(const ClothJobC *j, double *out) {
  btSoftBodyRigidBodyCollisionConfiguration cfg;
  btCollisionDispatcher disp(&cfg);
  btDbvtBroadphase bp;
  btSequentialImpulseConstraintSolver solver;
  btDefaultSoftBodySolver sbsolver;
  btSoftRigidDynamicsWorld world(&disp, &bp, &solver, &cfg, &sbsolver);
  const double T = frame_seconds();
  world.setGravity(btVector3(0, 0, -9.81 * j->gravity));
  btSoftBodyWorldInfo &wi = world.getWorldInfo();
  wi.m_gravity = btVector3(0, 0, -9.81 * j->gravity);
  wi.air_density = (btScalar)env("CLB_AIR", 1.2);
  wi.m_sparsesdf.Initialize();
  // static colliders
  std::vector<std::vector<btScalar>> cpos(j->ncol);
  std::vector<std::vector<int>> ctri(j->ncol);
  std::vector<btTriangleIndexVertexArray *> arrays;
  std::vector<btCollisionShape *> shapes;
  std::vector<btRigidBody *> bodies;
  for (int c = 0; c < j->ncol; ++c) {
    cpos[c].assign(j->col_pos[c], j->col_pos[c] + 3 * j->col_nv[c]);
    ctri[c].assign(j->col_tri[c], j->col_tri[c] + 3 * j->col_ntri[c]);
    if (j->col_ntri[c] == 0) continue;
    auto *arr = new btTriangleIndexVertexArray(j->col_ntri[c], ctri[c].data(), 3 * sizeof(int), j->col_nv[c], cpos[c].data(), 3 * sizeof(btScalar));
    auto *sh = new btBvhTriangleMeshShape(arr, true);
    sh->setMargin((btScalar)env("CLB_COLMARGIN", 0.002));
    btRigidBody::btRigidBodyConstructionInfo ci(0, nullptr, sh);
    ci.m_friction = (btScalar)env("CLB_COLFRIC", 1.0);
    auto *rb = new btRigidBody(ci);
    world.addRigidBody(rb);
    arrays.push_back(arr), shapes.push_back(sh), bodies.push_back(rb);
  }
  std::vector<btScalar> vp(j->pos, j->pos + 3 * j->nv);
  btSoftBody *sb = btSoftBodyHelpers::CreateFromTriMesh(wi, vp.data(), j->tri, j->ntri, false);
  btSoftBody::Material *mat = sb->m_materials[0];
  mat->m_kLST = (btScalar)env("CLB_LST", 0.9);
  mat->m_kAST = (btScalar)env("CLB_AST", 0.9);
  sb->generateBendingConstraints(2, mat);
  sb->m_cfg.piterations = (int)env("CLB_PITER", 10);
  sb->m_cfg.viterations = (int)env("CLB_VITER", 0);
  sb->m_cfg.kDF = (btScalar)env("CLB_FRIC", 0.8);
  sb->m_cfg.kDP = (btScalar)env("CLB_DAMP", 0.005);
  sb->m_cfg.kDG = 0;
  sb->m_cfg.kLF = 0;
  sb->m_cfg.kCHR = (btScalar)env("CLB_CHR", 1.0);
  sb->m_cfg.aeromodel = btSoftBody::eAeroModel::V_Point;
  sb->m_cfg.collisions = btSoftBody::fCollision::SDF_RS | (j->self_collision && env("CLB_SELF", 1.0) > 0.5 ? btSoftBody::fCollision::VF_SS : 0);
  if (j->pressure != 0) sb->m_cfg.kPR = (btScalar)(j->pressure * (1.0 / (T * T)) * env("CLB_PRESS", 1.0) * rest_volume(j));
  for (int i = 0; i < j->nv; ++i) sb->setMass(i, (j->pin && j->pin[i] >= 0.999) ? 0 : (btScalar)j->mass);
  sb->getCollisionShape()->setMargin((btScalar)(env("CLB_MARGIN", 1.0) * j->distance_min));
  world.addSoftBody(sb);
  const int sub = (int)env("CLB_SUB", 4);
  const int steps = std::max(0, j->end_frame - 1) * sub;
  const btScalar dt = (btScalar)(T / sub);
  const bool log = getenv("CLB_LOG") != nullptr;
  for (int s = 0; s < steps; ++s) {
    world.stepSimulation(dt, 0, dt);
    if (log && s % sub == 0) {
      btVector3 lo(1e9, 1e9, 1e9), hi(-1e9, -1e9, -1e9);
      for (int i = 0; i < sb->m_nodes.size(); ++i) lo.setMin(sb->m_nodes[i].m_x), hi.setMax(sb->m_nodes[i].m_x);
      fprintf(stderr, "CLB frame %d bounds (%.3f %.3f %.3f)-(%.3f %.3f %.3f) volume %.4f\n", s / sub, lo.x(), lo.y(), lo.z(), hi.x(), hi.y(), hi.z(), sb->getVolume());
    }
  }
  for (int i = 0; i < j->nv; ++i) {
    const btVector3 &x = sb->m_nodes[i].m_x;
    out[3 * i] = x.x(), out[3 * i + 1] = x.y(), out[3 * i + 2] = x.z();
  }
  world.removeSoftBody(sb);
  delete sb;
  for (auto *rb : bodies) world.removeRigidBody(rb), delete rb;
  for (auto *s : shapes) delete s;
  for (auto *a : arrays) delete a;
  return 0;
}
