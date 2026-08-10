{
  cacert,
  python314Packages,
  lib,
  rustPlatform,
  stdenv,
}:
let
  cargo_toml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = cargo_toml.package.name;
  version = cargo_toml.package.version;

  # The preview feature embeds files from plugin/ and the trmnl-framework
  # submodule via include_str!, so the build needs the whole repo as source.
  src = ../.;
  cargoRoot = "cli-client";
  buildAndTestSubdir = "cli-client";

  strictDeps = true;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  preCheck = ''
    export SSL_CERT_FILE=${cacert}/etc/ssl/certs/ca-bundle.crt
  '';

  meta = {
    homepage = cargo_toml.package.repository;
    license = lib.licenses.agpl3Plus;
  };
})
