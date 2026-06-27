cask "openless" do
  arch arm: "aarch64", intel: "x64"

  version "1.3.12"
  sha256 arm:   "d6cd007e7a08ae840e93a174c6a3b125608ca4da8d56305b8eac6d3137f02b74",
         intel: "82e67ee081206fefcc3cb93698308fc425737deca1fdac679862c1c3d6d602e6"

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
