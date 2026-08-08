{
  description = "Console plugin for TRMNL";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      eachSystem = nixpkgs.lib.genAttrs [
        "aarch64-linux"
        "x86_64-linux"
        "aarch64-darwin"
      ];
      eachSystemPkgs =
        fn:
        builtins.mapAttrs fn (
          eachSystem (
            system:
            nixpkgs.legacyPackages.${system} or (import nixpkgs {
              inherit system;
            })
          )
        );
      eachPkgs = fn: eachSystemPkgs (_: fn);
    in
    {
      packages = eachPkgs (pkgs: rec {
        trmnl-console = pkgs.callPackage ./cli-client/pkg.nix { };
        default = trmnl-console;
      });

      formatter = eachPkgs (pkgs: pkgs.nixfmt-tree);

      devShells = eachPkgs (pkgs: {
        default = pkgs.callPackage ./devshell.nix { };
      });
    };
}
