{
  lib,
  rustPlatform,
  makeDesktopItem,
  makeFontsConf,
  makeWrapper,
  pkg-config,
  fontconfig,
  libGL,
  libxkbcommon,
  migu,
  noto-fonts-cjk-sans,
  pipewire,
  playerctl,
  python3,
  vulkan-loader,
  wayland,
  xorg,
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
    categories = [ "AudioVideo" "Graphics" ];
  };

  runtimeLibraries = [
    fontconfig
    libGL
    libxkbcommon
    vulkan-loader
    wayland
    xorg.libX11
    xorg.libXcursor
    xorg.libXi
    xorg.libXrandr
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
    mkdir -p "$out/share/applications"
    cp -r ${desktopItem}/share/applications/* "$out/share/applications/"

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
    mainProgram = "neon-rain";
    platforms = lib.platforms.linux;
  };
}
