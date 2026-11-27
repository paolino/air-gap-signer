{ pkgs, rust, piCc, src }:
let platform = pkgs.makeRustPlatform { cargo = rust; rustc = rust; };
in platform.buildRustPackage {
  pname = "pi-panel-signer";
  version = "0.1.0";
  inherit src;
  cargoLock.lockFile = ../Cargo.lock;
  CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER = "${piCc.targetPrefix}cc";
  CC_aarch64_unknown_linux_musl = "${piCc.targetPrefix}cc";
  CFLAGS_aarch64_unknown_linux_musl = "-mno-outline-atomics";
  AR_aarch64_unknown_linux_musl = "${piCc.targetPrefix}ar";
  nativeBuildInputs = [ piCc ];
  buildPhase = ''
    runHook preBuild
    cargo build --frozen --release --target aarch64-unknown-linux-musl -p signer-pi --bin signer-pi --bin signer-boot-status -j "$NIX_BUILD_CORES"
    runHook postBuild
  '';
  doCheck = false; # Hardware executable; native workspace tests run with just ci.
  dontStrip = true;
  installPhase = "install -Dm755 -t $out/bin target/aarch64-unknown-linux-musl/release/{signer-pi,signer-boot-status}";
}
