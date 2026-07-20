{
  description = "Neon Rain — a living, music-reactive Matrix rain visualizer";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: import nixpkgs { inherit system; };
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.callPackage ./package.nix { };
          neon-rain = self.packages.${system}.default;
        });

      apps = forAllSystems (system:
        let
          app = {
            type = "app";
            program = "${self.packages.${system}.default}/bin/neon-rain";
            meta = {
              description = "Launch the Neon Rain visualizer";
            };
          };
        in
        {
          default = app;
          neon-rain = app;
        });

      checks = forAllSystems (system: {
        package = self.packages.${system}.default;
      });

      devShells = forAllSystems (system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.default ];

            packages = with pkgs; [
              cargo
              clippy
              git
              python3
              rustc
              rustfmt
            ];

            RUST_BACKTRACE = "1";
          };
        });
    };
}
