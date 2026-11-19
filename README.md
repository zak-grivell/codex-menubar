# Codex Menu Bar

A small native menu bar app for Codex usage on Apple silicon Macs. Click the gauge to see how much of your 5-hour and weekly limits remains and when each resets in your local time. It fetches on launch and every five minutes. The menu also has a manual refresh.

## Run and install

Use a recent [Codex CLI](https://learn.chatgpt.com/docs/cli) signed in to your ChatGPT account. If needed, run `codex login` first.

```sh
nix run github:zak-grivell/codex-menubar
```

To install the `codex-menubar` command in your user profile:

```sh
nix profile add github:zak-grivell/codex-menubar
codex-menubar
```

To build the macOS app bundle:

```sh
nix build github:zak-grivell/codex-menubar
open "result/Applications/Codex Menu Bar.app"
```

The app checks `PATH` and common Homebrew, Nix, and user bin directories for `codex`. For a custom install, specify the executable when launching from a terminal:

```sh
CODEX_BIN=/absolute/path/to/codex nix run github:zak-grivell/codex-menubar
```

The app uses the CLI's [Codex app-server](https://learn.chatgpt.com/docs/app-server) and existing sign-in. Each refresh starts a short-lived CLI process in a background thread. If a refresh fails, the menu keeps the last successful reading and displays the error.

## nix-darwin

Add the flake input:

```nix
inputs.codex-menubar.url = "github:zak-grivell/codex-menubar";
```

Add `codex-menubar` to your `outputs` arguments and append this module to your Mac's existing `darwinSystem.modules` list:

```nix
{
  environment.systemPackages = [ codex-menubar.packages.aarch64-darwin.default ];
}
```

Run your usual `darwin-rebuild switch` command, then launch `codex-menubar`.
