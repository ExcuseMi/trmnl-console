{
  mkShell,
  lib,

  clang,
  llvmPackages_latest,
  rustup,
  openssl,
  pkg-config,
}:
let
  overrides = (builtins.fromTOML (builtins.readFile ./rust-toolchain.toml));
  extraLibs = [ ];
  libPath = lib.makeLibraryPath extraLibs;
in
mkShell {
  RUSTC_VERSION = overrides.toolchain.channel;
  # https://github.com/rust-lang/rust-bindgen#environment-variables
  LIBCLANG_PATH = lib.makeLibraryPath [ llvmPackages_latest.libclang.lib ];
  shellHook = ''
    export PATH=$PATH:''${CARGO_HOME:-~/.cargo}/bin
    export PATH=$PATH:''${RUSTUP_HOME:-~/.rustup}/toolchains/$RUSTC_VERSION-x86_64-unknown-linux-gnu/bin/
  '';
  # Add precompiled library to rustc search path
  RUSTFLAGS = (
    builtins.map (a: "-L ${a}/lib") [
      # add libraries here (e.g. pkgs.libvmi)
    ]
  );
  LD_LIBRARY_PATH = libPath;
  # Add glibc, clang, glib, and other headers to bindgen search path
  BINDGEN_EXTRA_CLANG_ARGS =
    # Includes normal include path
    (builtins.map (a: ''-I"${a}/include"'') [
      # add dev libraries here (e.g. pkgs.libvmi.dev)
    ])
    # Includes with special directory paths
    ++ [
      ''-I"${llvmPackages_latest.libclang.lib}/lib/clang/${llvmPackages_latest.libclang.version}/include"''
    ];

  nativeBuildInputs = [ ];

  buildInputs = [
    clang
    llvmPackages_latest.bintools
    rustup
    openssl
    pkg-config
  ];
}
