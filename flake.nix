{
  description = "fran db";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    nixpkgs,
    crane,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachSystem ["x86_64-linux" "aarch64-linux"] (
      system: let
        pkgs = nixpkgs.legacyPackages.${system};
        lib = pkgs.lib;
        craneLib = crane.mkLib pkgs;

        nativeBuildInputs = with pkgs; [
          pkg-config
          wrapGAppsHook3
        ];

        buildInputs = with pkgs; [
          webkitgtk_4_1
          gtk3
          libsoup_3
          glib-networking
          dbus
          openssl
        ];

        frontend = pkgs.buildNpmPackage {
          pname = "fran-db-frontend";
          version = "0.1.0";
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.difference ./. ./src-tauri;
          };
          npmDeps = pkgs.importNpmLock {npmRoot = ./.;};
          npmConfigHook = pkgs.importNpmLock.npmConfigHook;
          env.VITE_NIX_BUILD = "1";
          installPhase = "cp -r dist $out";
        };

        commonArgs = {
          src = ./.;
          cargoToml = ./src-tauri/Cargo.toml;
          cargoLock = ./src-tauri/Cargo.lock;
          postUnpack = ''
            cd $sourceRoot/src-tauri
            sourceRoot=.
          '';
          strictDeps = true;
          inherit nativeBuildInputs buildInputs;
          cargoExtraArgs = "--locked --features tauri/custom-protocol";
        };

        fran-db = craneLib.buildPackage (
          commonArgs
          // {
            cargoArtifacts = craneLib.buildDepsOnly commonArgs;
            doCheck = false;
            preBuild = ''
              cp -r ${frontend} ../dist
            '';
            # webkitgtk's DMA-BUF renderer hits Wayland protocol errors on NVIDIA
            preFixup = ''
              gappsWrapperArgs+=(--set-default WEBKIT_DISABLE_DMABUF_RENDERER 1)
            '';
            postInstall = ''
              install -Dm644 icons/32x32.png $out/share/icons/hicolor/32x32/apps/fran-db.png
              install -Dm644 icons/128x128.png $out/share/icons/hicolor/128x128/apps/fran-db.png
              install -Dm644 icons/128x128@2x.png $out/share/icons/hicolor/256x256/apps/fran-db.png
              install -Dm644 ${desktopItem}/share/applications/*.desktop -t $out/share/applications
            '';
            meta = {
              description = "A desktop SQL client";
              license = lib.licenses.gpl3Plus;
              mainProgram = "fran-db";
              platforms = lib.platforms.linux;
            };
          }
        );

        desktopItem = pkgs.makeDesktopItem {
          name = "fran-db";
          desktopName = "Fran DB";
          exec = "fran-db";
          icon = "fran-db";
          comment = "A desktop SQL client";
          categories = ["Development" "Database"];
        };
      in {
        packages.default = fran-db;

        devShells.default = craneLib.devShell {
          inputsFrom = [fran-db];
          packages = with pkgs; [
            rust-analyzer
            rustfmt
            clippy
            nodejs_26
          ];
          GIO_MODULE_DIR = "${pkgs.glib-networking}/lib/gio/modules";
          WEBKIT_DISABLE_DMABUF_RENDERER = 1;
        };
      }
    );
}
