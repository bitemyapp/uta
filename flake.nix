{
  description = "Uta 歌: native YouTube Music for Aizome";

  inputs = {
    nixpkgs.follows = "aizome/nixpkgs";
    aizome.url = "github:bitemyapp/aizome/729cf1c140b349dc6f8a9d7ba1772c4b9bb4fe18";
    aizome.inputs.sidestep.url = "github:bitemyapp/sidestep/16a1e376c0bd85d52ef020e114bbd1707fb9b14b";
  };

  outputs = { self, nixpkgs, aizome }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      each = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      pythonFor = pkgs: pkgs.python3.withPackages (ps: [ ps.ytmusicapi ps.yt-dlp ps.secretstorage ]);
    in {
      packages = each (pkgs: rec {
        uta = pkgs.callPackage ./nix/package.nix {
          src = pkgs.lib.cleanSource ./.;
          inherit aizome;
          sidestep = aizome.inputs.sidestep;
          python = pythonFor pkgs;
          fonts = aizome.packages.${pkgs.stdenv.hostPlatform.system}.fonts;
        };
        default = uta;
      });
      apps = each (pkgs: {
        default = { type = "app"; program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.uta}/bin/aizome-uta"; };
      });
      devShells = each (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [ cargo rustc rustfmt clippy pkg-config mpv deno dbus (pythonFor pkgs) ];
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [ pkgs.fontconfig pkgs.wayland ];
          AIZOME_FONT_DIR = aizome.packages.${pkgs.stdenv.hostPlatform.system}.fonts;
        };
      });
      checks = each (pkgs: { inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) uta; });
      nixosModules.default = { config, lib, pkgs, ... }: {
        options.programs.uta.enable = lib.mkEnableOption "Uta, the native YouTube Music client for Aizome";
        config = lib.mkIf config.programs.uta.enable {
          environment.systemPackages = [ self.packages.${pkgs.stdenv.hostPlatform.system}.uta ];
        };
      };
    };
}
