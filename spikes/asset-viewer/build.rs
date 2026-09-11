fn main() {
    const HOMEBREW_PREFIX: &str = "/opt/homebrew/opt";

    println!("cargo:rustc-link-search=native={HOMEBREW_PREFIX}/stormlib/lib");
    println!("cargo:rustc-link-lib=dylib=storm");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{HOMEBREW_PREFIX}/stormlib/lib");

    println!("cargo:rustc-link-search=native={HOMEBREW_PREFIX}/sdl3/lib");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{HOMEBREW_PREFIX}/sdl3/lib");
}
