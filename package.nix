# Local package definition for forgemux
{
  lib,
  rustPlatform,
  pkg-config,
  cmake,
  openssl,
  libgit2,
  zlib,
  git,
}:

rustPlatform.buildRustPackage {
  pname = "forgemux";
  version = "0.1.8";

  src = ./.;

  # Generate with `cargo generate-lockfile`
  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = [
    pkg-config
    cmake
    git
  ];

  buildInputs = [
    openssl
    libgit2
    zlib
    git
  ];

  env = {
    OPENSSL_NO_VENDOR = "1";
    LIBGIT2_NO_VENDOR = "1";
  };

  meta = {
    description = "Forgemux - durable agent session manager";
    license = with lib.licenses; [ mit ];
    platforms = lib.platforms.unix;
    mainProgram = "fmux";
  };
}
