class Openless < Formula
  desc "Menu-bar voice input layer for macOS"
  homepage "https://github.com/appergb/openless"
  url "https://github.com/appergb/openless/archive/refs/tags/v1.2.20-tauri.tar.gz"
  sha256 "be1363f7762a42b7e932ec3cc687cfdfb5b550b6cf1c96d52a1f87c906f27863"
  license "MIT"

  head "https://github.com/appergb/openless.git", branch: "main"

  depends_on "rust" => :build
  depends_on "node" => :build
  depends_on :macos
  depends_on arch: :arm64

  # qwen-asr submodule for local ASR engine on macOS
  resource "qwen-asr" do
    url "https://github.com/antirez/qwen-asr.git",
        revision: "b00b789b17051aea61e9717458171100662318a4"
  end

  def install
    # Place submodule at expected path before building
    vendor_dir = "openless-all/app/src-tauri/vendor/qwen-asr"
    rm_rf vendor_dir
    resource("qwen-asr").stage(vendor_dir)

    cd "openless-all/app" do
      # Install frontend dependencies
      system "npm", "ci"

      # Build Tauri app with ad-hoc signing (no Developer ID required)
      ENV["APPLE_SIGNING_IDENTITY"] = "-"
      system "npm", "run", "tauri", "--", "build"

      # Install the .app bundle
      prefix.install "src-tauri/target/release/bundle/macos/OpenLess.app"
    end

    # Symlink binary for command-line access
    bin.install_symlink prefix/"OpenLess.app/Contents/MacOS/openless"
  end

  def caveats
    <<~EOS
      OpenLess.app has been installed to:
        #{prefix}/OpenLess.app

      To use it, link it into /Applications:
        ln -sf #{opt_prefix}/OpenLess.app /Applications/OpenLess.app

      Or launch it directly:
        open #{opt_prefix}/OpenLess.app

      First-time setup:
        - Grant Accessibility permission when prompted
        - Grant Microphone permission when prompted

      If Gatekeeper blocks the app (ad-hoc signed), run:
        xattr -cr /Applications/OpenLess.app
    EOS
  end

  test do
    assert_path_exists prefix/"OpenLess.app"
    assert_predicate prefix/"OpenLess.app/Contents/MacOS/openless", :executable?
  end
end
