cask "openless" do
  arch arm: "aarch64", intel: "x64"

  version "1.3.14"
  sha256 arm:   "4bfa85f48714626ec010b92d22a5ab98c834f60b5c7e5f6281e12a11ad90ad9f",
         intel: "929ad6c047fc8942724b7e1edb5dc0d88affbd2ec4184b2dd3d482c0e06d999f"

  url "https://github.com/appergb/openless/releases/download/v#{version}-tauri/OpenLess_#{version}_#{arch}.dmg"
  name "OpenLess"
  desc "Menu-bar voice input layer"
  homepage "https://github.com/appergb/openless"

  livecheck do
    url :url
    regex(/^v?(\d+(?:\.\d+)+)[._-]tauri$/i)
  end

  auto_updates true
  depends_on macos: :monterey

  app "OpenLess.app"

  zap trash: [
    "~/Library/Application Support/OpenLess",
    "~/Library/Caches/com.openless.app",
    "~/Library/Logs/OpenLess",
    "~/Library/Preferences/com.openless.app.plist",
    "~/Library/WebKit/com.openless.app",
  ]
end
