{
  python314Packages,
  lib,
  rustPlatform,
  stdenv
}:
let
  cargo_toml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = cargo_toml.package.name;
  version = cargo_toml.package.version;

  src = ./.;

  strictDeps = true;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  meta = {
    homepage = cargo_toml.package.repository;
    license = lib.licenses.agpl3Plus;
  };
})
