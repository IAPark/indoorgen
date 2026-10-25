//! Reader for python-side dumps written by tools/bedhook.py + npz2dir.py (raw little-endian arrays + index.json).
use crate::mesh::V3;
use std::path::PathBuf;

pub struct PyDump {
    pub dir: PathBuf,
    pub index: serde_json::Value,
}

pub struct PyMesh {
    pub v: Vec<V3>,
    pub faces: Vec<Vec<u32>>,
    pub pin: Vec<f64>,
}

impl PyDump {
    pub fn load(dir: &str) -> PyDump {
        let dir = PathBuf::from(dir);
        let index = serde_json::from_str(&std::fs::read_to_string(dir.join("index.json")).expect("dump index")).unwrap();
        PyDump { dir, index }
    }
    pub fn has(&self, key: &str) -> bool {
        self.index.get(key).is_some()
    }
    pub fn str(&self, key: &str) -> String {
        self.index[key][1].as_str().unwrap_or("").to_string()
    }
    pub fn json(&self, key: &str) -> serde_json::Value {
        serde_json::from_str(&self.str(key)).unwrap_or(serde_json::Value::Null)
    }
    pub fn f64s(&self, key: &str) -> Vec<f64> {
        let b = std::fs::read(self.dir.join(format!("{key}.bin"))).unwrap_or_else(|_| panic!("missing {key}"));
        b.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()
    }
    pub fn i32s(&self, key: &str) -> Vec<i32> {
        let b = std::fs::read(self.dir.join(format!("{key}.bin"))).unwrap_or_else(|_| panic!("missing {key}"));
        b.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()
    }
    /// mesh stored as <prefix>.v/.lens/.idx[/.pin]
    pub fn mesh(&self, prefix: &str) -> PyMesh {
        let f = self.f64s(&format!("{prefix}.v"));
        let v: Vec<V3> = f.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
        let lens = self.i32s(&format!("{prefix}.lens"));
        let idx = self.i32s(&format!("{prefix}.idx"));
        let mut faces = vec![];
        let mut o = 0;
        for l in lens {
            faces.push(idx[o..o + l as usize].iter().map(|&i| i as u32).collect());
            o += l as usize;
        }
        let pin = if self.has(&format!("{prefix}.pin")) { self.f64s(&format!("{prefix}.pin")) } else { vec![] };
        PyMesh { v, faces, pin }
    }
}
