class CodexRouter < Formula
  desc "Local account and quota router for Codex CLI"
  homepage "https://github.com/ShravanSunder/codex-router"
  # Source commit: be7e61e8bca32ebac172eb743e8064d2c7d6ffe7
  url "https://github.com/ShravanSunder/codex-router/releases/download/v0.1.102/codex-router-v0.1.102-aarch64-apple-darwin.tar.gz"
  sha256 "54e978fb41bab35653a998ffdc50a9f4c49e83240fe5458565072e575d4c1517"
  license any_of: ["MIT", "Apache-2.0"]

  depends_on arch: :arm64
  depends_on :macos

  def install
    bin.install "codex-router"
    bin.install "agent-collaboration"
    bin.install "agent-sessions"
    bin.install "codex-router-board-upgrade"
  end

  test do
    assert_match "codex-router #{version}", shell_output("#{bin}/codex-router --version")
  end
end
