fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    caligo_core::commands::media::entry(&args, "caligo-media");
}
