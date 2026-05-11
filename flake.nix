{
  description = "Runaway: a code-first durable execution engine";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    crate2nix.url = "github:nix-community/crate2nix";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ ./rust.nix ];
      systems = [ "x86_64-linux" ];

      rust.toolchain.channel = "stable";
      rust.workspace = {
        cargo-nix = ./Cargo.nix;
        default = members: members.runaway;
      };
    };
}
