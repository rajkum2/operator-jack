# Homebrew formula for Operator Jack
# To use: brew install rajkum2/tap/operator-jack
#
# This file is a template. The actual formula lives in the
# rajkum2/homebrew-tap repository. The release workflow (release.yml) now
# includes a tap-update step that bumps the tap's url/version/sha256 on each
# tagged release; keep the version/url below in sync when updating this template.

class OperatorJack < Formula
  desc "Local-first CLI for deterministic macOS automation"
  homepage "https://github.com/rajkum2/operator-jack"
  url "https://github.com/rajkum2/operator-jack/releases/download/v0.7.1/operator-jack-v0.7.1-macos-universal.tar.gz"
  sha256 "PLACEHOLDER_SHA256"
  license "MIT"
  version "0.7.1"

  depends_on :macos

  def install
    bin.install "operator-jack"
    bin.install "operator-macos-helper"
  end

  def caveats
    <<~EOS
      To use UI automation features, grant Accessibility permission to your
      terminal application:

        System Settings > Privacy & Security > Accessibility
        Add: #{which_terminal}

      Then restart your terminal and run:

        operator-jack doctor
    EOS
  end

  def which_terminal
    case ENV["TERM_PROGRAM"]
    when "iTerm.app" then "iTerm2"
    when "Apple_Terminal" then "Terminal.app"
    when "vscode" then "Visual Studio Code"
    else "your terminal app (Terminal.app, iTerm2, etc.)"
    end
  end

  test do
    assert_match "operator-jack", shell_output("#{bin}/operator-jack --version")
  end
end
