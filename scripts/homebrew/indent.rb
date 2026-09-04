class Indent < Formula
  desc "Simple, readable, beginner-friendly programming language — Xytro Labs"
  homepage "https://github.com/xytro-labs/indent"
  version "2.1.0"
  license "MIT"

  if OS.mac? && Hardware::CPU.arm?
    url "https://github.com/xytro-labs/indent/releases/latest/download/indent-v#{version}-aarch64-apple-darwin.tar.gz"
    sha256 "0000000000000000000000000000000000000000000000000000000000000000" # updated on release
  elsif OS.mac? && Hardware::CPU.intel?
    url "https://github.com/xytro-labs/indent/releases/latest/download/indent-v#{version}-x86_64-apple-darwin.tar.gz"
    sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  elsif OS.linux? && Hardware::CPU.intel?
    url "https://github.com/xytro-labs/indent/releases/latest/download/indent-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  elsif OS.linux? && Hardware::CPU.arm?
    url "https://github.com/xytro-labs/indent/releases/latest/download/indent-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
    sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  end

  def install
    bin.install "indent/bin/indent" => "indent"
    libexec.install Dir["indent/std/"]
    libexec.install Dir["indent/packages/"] if Dir.exist?("indent/packages")
    (libexec/"bin").install "indent/bin/indent"

    # Create wrapper that sets INDENT_PATH
    (bin/"indent").write_env_script libexec/"bin/indent",
      INDENT_PATH: "#{libexec}"
  end

  test do
    assert_match "indent #{version}", shell_output("#{bin}/indent --version")
    assert_match "Hello", pipe_output("#{bin}/indent -", 'say "Hello"')
  end
end
