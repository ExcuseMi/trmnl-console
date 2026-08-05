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
    {
      devShells.x86_64-linux =
        let
          pkgs = import nixpkgs { system = "x86_64-linux"; };
        in
        {
          default = pkgs.callPackage ./devshell.nix { };
        };
    };
}
