{
  rust-bin,
  lib,
  stdenv,
}:
rust-bin.stable.latest.default.override {
  targets =
    lib.optionals stdenv.isDarwin [
      "x86_64-apple-darwin"
      "aarch64-apple-darwin"
    ]
    ++ lib.optionals stdenv.isLinux [
      "x86_64-unknown-linux-musl"
      "aarch64-unknown-linux-musl"
    ];
}
