fn main() {
    for s in ["509542_TVStandFactory", "a", "living-room_0/0"] { println!("{}", infinigen::pyhash::py_str_hash(s)); }
}
