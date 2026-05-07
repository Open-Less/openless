cask "openless" do
  arch arm: "aarch64", intel: "x64"

  version "1.2.22"
  sha256 arm:   "8e65729eb671f5fec3ce392d4f8dfd33303f1c84f8f4533683f8d3a3afd1e21d",
         intel: "6d97fb7cab173b4f3241aa18b3db0f51a03a03d6b2bdea48cab8fe761440df7b"

  url "https://github.com/appergb/openless/releases/download/v#{version}-tauri/OpenLess_#{version}_#{arch}.dmg"
  name "OpenLess"
  desc "Menu-bar voice input layer for macOS"
  homepage "https://github.com/appergb/openless"

  livecheck do
    url :url
    regex(/^v?(\d+(?:\.\d+)+)[._-]tauri$/i)
  end

  auto_updates true

  app "OpenLess.app"

  zap trash: [
    "~/Library/Application Support/OpenLess",
    "~/Library/Caches/com.openless.app",
    "~/Library/Logs/OpenLess",
    "~/Library/Preferences/com.openless.app.plist",
    "~/Library/WebKit/com.openless.app",
  ]
end
