// Remove this or modify succinct to also use this method for compiling the host
//
/// Build script to automatically build the guest program
fn main() {
    zkm_build::build_program("../program"); // Here program is the directory where the guest program is located
}
