// gen2: generate a house (see src/gen2cli.rs for the command line)
fn main() {
    indoorgen::gen2cli::run(std::env::args().collect(), false);
}
