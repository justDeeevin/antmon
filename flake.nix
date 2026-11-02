{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ (import rust-overlay) ];
        config.allowUnsupportedSystem = true;
      };
      mingw = pkgs.pkgsCross.mingwW64;
      dylibs = with pkgs; [
        libx11
        libxi
        libxtst

        libxkbcommon
        libinput
      ];
    in
    {
      devShell.${system} = pkgs.mkShell {
        packages = with pkgs; [
          (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
          pkg-config
          mingw.stdenv.cc
          wine64
        ];
        buildInputs = [ mingw.windows.pthreads ] ++ dylibs;
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath dylibs;
      };
    };
}
