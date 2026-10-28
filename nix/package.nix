{ lib, rustPlatform, runCommand, makeWrapper, mpv, deno, dbus, fontconfig, wayland,
  src, aizome, sidestep, python, fonts, enableRustyPipe ? true }:
let
  source = runCommand "uta-source" { } ''
    mkdir -p $out
    cp -r ${src} $out/uta
    cp -r ${aizome} $out/aizome
    cp -r ${sidestep} $out/sidestep-aizome
    chmod u+w $out/aizome
    ln -sfn ../sidestep-aizome $out/aizome/sidestep
  '';
in rustPlatform.buildRustPackage {
  pname = "aizome-uta";
  version = "0.1.0";
  src = source;
  sourceRoot = "uta-source/uta";
  cargoLock.lockFile = ../Cargo.lock;
  buildNoDefaultFeatures = true;
  buildFeatures = lib.optionals enableRustyPipe [ "rustypipe" ];
  nativeBuildInputs = [ makeWrapper python dbus ];
  AIZOME_FONT_DIR = fonts;
  SIDESTEP_BACKEND = "null";
  LD_LIBRARY_PATH = lib.makeLibraryPath [ fontconfig wayland ];
  UTA_MPV = "${mpv}/bin/mpv";
  postCheck = ''
    python3 -m unittest discover -s service/tests -v
  '';
  postInstall = ''
    ${lib.optionalString enableRustyPipe ''
      install -Dm644 vendor/rustypipe/LICENSE $out/share/doc/aizome-uta/LICENSE-RustyPipe
      install -Dm644 vendor/rustypipe/ejs/LICENSE $out/share/doc/aizome-uta/LICENSE-EJS
    ''}
    install -Dm644 data/org.aizome.Uta.desktop $out/share/applications/org.aizome.Uta.desktop
    install -Dm644 data/org.aizome.Uta.svg $out/share/icons/hicolor/scalable/apps/org.aizome.Uta.svg
    wrapProgram $out/bin/aizome-uta \
      --set UTA_PYTHON ${python}/bin/python3 \
      --set UTA_MPV ${mpv}/bin/mpv \
      --prefix PATH : ${lib.makeBinPath [ deno ]} \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath [ fontconfig wayland ]} \
      --set-default AIZOME_FONT_DIR ${fonts}
  '';
  meta = {
    description = "Native YouTube Music client for Aizome";
    homepage = "https://github.com/bitemyapp/uta";
    license = if enableRustyPipe then lib.licenses.gpl3Only else with lib.licenses; [ mit asl20 ];
    platforms = lib.platforms.linux;
    mainProgram = "aizome-uta";
  };
}
