{
  lib,
  rustPlatform,
  makeDesktopItem,
  makeFontsConf,
  makeWrapper,
  pkg-config,
  fontconfig,
  libGL,
  libx11,
  libxcursor,
  libxi,
  libxkbcommon,
  libxrandr,
  migu,
  noto-fonts-cjk-sans,
  pipewire,
  playerctl,
  python3,
  vulkan-loader,
  wayland,
}:

let
  cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);

  fontsConf = makeFontsConf {
    fontDirectories = [
      migu
      noto-fonts-cjk-sans
    ];
  };

  desktopItem = makeDesktopItem {
    name = "neon-rain";
    desktopName = "Neon Rain";
    comment = "Living, music-reactive Matrix rain";
    exec = "neon-rain";
    terminal = false;
    icon = "neon-rain";
    categories = [ "Graphics" ];
  };

  runtimeLibraries = [
    fontconfig
    libGL
    libx11
    libxcursor
    libxi
    libxkbcommon
    libxrandr
    vulkan-loader
    wayland
  ];
in
rustPlatform.buildRustPackage {
  pname = cargoToml.package.name;
  version = cargoToml.package.version;

  src = lib.cleanSource ./.;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = [
    makeWrapper
    pkg-config
  ];

  buildInputs = runtimeLibraries;

  postInstall = ''
    mkdir -p \
      "$out/share/applications" \
      "$out/share/pixmaps" \
      "$out/share/neon-rain"

    cp -r ${desktopItem}/share/applications/* "$out/share/applications/"
    cp docs/assets/neon-rain-social-preview.png \
      "$out/share/pixmaps/neon-rain.png"
    cp config/neon-rain.conf \
      "$out/share/neon-rain/config.example.conf"
    install -m 0755 scripts/capture-neon-rain.sh \
      "$out/bin/neon-rain-capture"

    wrapProgram "$out/bin/neon-rain" \
      --prefix PATH : ${lib.makeBinPath [
        pipewire
        playerctl
        python3
      ]} \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibraries} \
      --set FONTCONFIG_FILE ${fontsConf}
  '';

  meta = {
    description = "Living, music-reactive Matrix rain visualizer";
    homepage = "https://github.com/Yearbook-enzyme/neon-rain";
    license = lib.licenses.mit;
    mainProgram = "neon-rain";
    platforms = lib.platforms.linux;
  };
}
