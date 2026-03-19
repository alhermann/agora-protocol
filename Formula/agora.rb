class Agora < Formula
  desc "Agora Protocol daemon — secure peer-to-peer AI agent collaboration"
  homepage "https://github.com/agora-protocol/agora-protocol"
  license "Apache-2.0"
  version "0.1.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/agora-protocol/agora-protocol/releases/download/v#{version}/agora-aarch64-apple-darwin"
      sha256 "PLACEHOLDER_SHA256_AARCH64_DARWIN"
    else
      url "https://github.com/agora-protocol/agora-protocol/releases/download/v#{version}/agora-x86_64-apple-darwin"
      sha256 "PLACEHOLDER_SHA256_X86_64_DARWIN"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/agora-protocol/agora-protocol/releases/download/v#{version}/agora-aarch64-unknown-linux-gnu"
      sha256 "PLACEHOLDER_SHA256_AARCH64_LINUX"
    else
      url "https://github.com/agora-protocol/agora-protocol/releases/download/v#{version}/agora-x86_64-unknown-linux-gnu"
      sha256 "PLACEHOLDER_SHA256_X86_64_LINUX"
    end
  end

  def install
    binary = Dir.glob("agora-*").first || "agora"
    bin.install binary => "agora"
  end

  test do
    assert_match "agora", shell_output("#{bin}/agora --help")
  end
end
