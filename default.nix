{ pkgs ? import <nixpkgs> {} }:

{
  forgemux = pkgs.callPackage ./package.nix { };
}
