
export const platformArchTriples = {
  "win32": {
    "arm64": [
      {
        "triple": "aarch64-pc-windows-msvc\r",
        "platformArchABI": "win32-arm64-msvc\r",
        "platform": "win32",
        "arch": "arm64",
        "abi": "msvc\r"
      }
    ],
    "ia32": [
      {
        "triple": "i686-pc-windows-gnu\r",
        "platformArchABI": "win32-ia32-gnu\r",
        "platform": "win32",
        "arch": "ia32",
        "abi": "gnu\r"
      },
      {
        "triple": "i686-pc-windows-msvc\r",
        "platformArchABI": "win32-ia32-msvc\r",
        "platform": "win32",
        "arch": "ia32",
        "abi": "msvc\r"
      }
    ],
    "x64": [
      {
        "triple": "x86_64-pc-windows-gnu\r",
        "platformArchABI": "win32-x64-gnu\r",
        "platform": "win32",
        "arch": "x64",
        "abi": "gnu\r"
      },
      {
        "triple": "x86_64-pc-windows-msvc\r",
        "platformArchABI": "win32-x64-msvc\r",
        "platform": "win32",
        "arch": "x64",
        "abi": "msvc\r"
      }
    ]
  },
  "linux": {
    "arm64": [
      {
        "triple": "aarch64-unknown-linux-gnu\r",
        "platformArchABI": "linux-arm64-gnu\r",
        "platform": "linux",
        "arch": "arm64",
        "abi": "gnu\r"
      },
      {
        "triple": "aarch64-unknown-linux-musl\r",
        "platformArchABI": "linux-arm64-musl\r",
        "platform": "linux",
        "arch": "arm64",
        "abi": "musl\r"
      }
    ],
    "arm": [
      {
        "triple": "arm-unknown-linux-gnueabi\r",
        "platformArchABI": "linux-arm-gnueabi\r",
        "platform": "linux",
        "arch": "arm",
        "abi": "gnueabi\r"
      },
      {
        "triple": "arm-unknown-linux-gnueabihf\r",
        "platformArchABI": "linux-arm-gnueabihf\r",
        "platform": "linux",
        "arch": "arm",
        "abi": "gnueabihf\r"
      },
      {
        "triple": "arm-unknown-linux-musleabi\r",
        "platformArchABI": "linux-arm-musleabi\r",
        "platform": "linux",
        "arch": "arm",
        "abi": "musleabi\r"
      },
      {
        "triple": "arm-unknown-linux-musleabihf\r",
        "platformArchABI": "linux-arm-musleabihf\r",
        "platform": "linux",
        "arch": "arm",
        "abi": "musleabihf\r"
      },
      {
        "triple": "armv7-unknown-linux-gnueabi\r",
        "platformArchABI": "linux-arm-gnueabi\r",
        "platform": "linux",
        "arch": "arm",
        "abi": "gnueabi\r"
      },
      {
        "triple": "armv7-unknown-linux-gnueabihf\r",
        "platformArchABI": "linux-arm-gnueabihf\r",
        "platform": "linux",
        "arch": "arm",
        "abi": "gnueabihf\r"
      },
      {
        "triple": "armv7-unknown-linux-musleabi\r",
        "platformArchABI": "linux-arm-musleabi\r",
        "platform": "linux",
        "arch": "arm",
        "abi": "musleabi\r"
      },
      {
        "triple": "armv7-unknown-linux-musleabihf\r",
        "platformArchABI": "linux-arm-musleabihf\r",
        "platform": "linux",
        "arch": "arm",
        "abi": "musleabihf\r"
      }
    ],
    "armv5te": [
      {
        "triple": "armv5te-unknown-linux-gnueabi\r",
        "platformArchABI": "linux-armv5te-gnueabi\r",
        "platform": "linux",
        "arch": "armv5te",
        "abi": "gnueabi\r"
      },
      {
        "triple": "armv5te-unknown-linux-musleabi\r",
        "platformArchABI": "linux-armv5te-musleabi\r",
        "platform": "linux",
        "arch": "armv5te",
        "abi": "musleabi\r"
      }
    ],
    "ia32": [
      {
        "triple": "i686-unknown-linux-gnu\r",
        "platformArchABI": "linux-ia32-gnu\r",
        "platform": "linux",
        "arch": "ia32",
        "abi": "gnu\r"
      },
      {
        "triple": "i686-unknown-linux-musl\r",
        "platformArchABI": "linux-ia32-musl\r",
        "platform": "linux",
        "arch": "ia32",
        "abi": "musl\r"
      }
    ],
    "mips": [
      {
        "triple": "mips-unknown-linux-gnu\r",
        "platformArchABI": "linux-mips-gnu\r",
        "platform": "linux",
        "arch": "mips",
        "abi": "gnu\r"
      },
      {
        "triple": "mips-unknown-linux-musl\r",
        "platformArchABI": "linux-mips-musl\r",
        "platform": "linux",
        "arch": "mips",
        "abi": "musl\r"
      }
    ],
    "mips64": [
      {
        "triple": "mips64-unknown-linux-gnuabi64\r",
        "platformArchABI": "linux-mips64-gnuabi64\r",
        "platform": "linux",
        "arch": "mips64",
        "abi": "gnuabi64\r"
      },
      {
        "triple": "mips64-unknown-linux-muslabi64\r",
        "platformArchABI": "linux-mips64-muslabi64\r",
        "platform": "linux",
        "arch": "mips64",
        "abi": "muslabi64\r"
      }
    ],
    "mips64el": [
      {
        "triple": "mips64el-unknown-linux-gnuabi64\r",
        "platformArchABI": "linux-mips64el-gnuabi64\r",
        "platform": "linux",
        "arch": "mips64el",
        "abi": "gnuabi64\r"
      },
      {
        "triple": "mips64el-unknown-linux-muslabi64\r",
        "platformArchABI": "linux-mips64el-muslabi64\r",
        "platform": "linux",
        "arch": "mips64el",
        "abi": "muslabi64\r"
      }
    ],
    "mipsel": [
      {
        "triple": "mipsel-unknown-linux-gnu\r",
        "platformArchABI": "linux-mipsel-gnu\r",
        "platform": "linux",
        "arch": "mipsel",
        "abi": "gnu\r"
      },
      {
        "triple": "mipsel-unknown-linux-musl\r",
        "platformArchABI": "linux-mipsel-musl\r",
        "platform": "linux",
        "arch": "mipsel",
        "abi": "musl\r"
      }
    ],
    "powerpc": [
      {
        "triple": "powerpc-unknown-linux-gnu\r",
        "platformArchABI": "linux-powerpc-gnu\r",
        "platform": "linux",
        "arch": "powerpc",
        "abi": "gnu\r"
      }
    ],
    "powerpc64": [
      {
        "triple": "powerpc64-unknown-linux-gnu\r",
        "platformArchABI": "linux-powerpc64-gnu\r",
        "platform": "linux",
        "arch": "powerpc64",
        "abi": "gnu\r"
      }
    ],
    "ppc64": [
      {
        "triple": "powerpc64le-unknown-linux-gnu\r",
        "platformArchABI": "linux-ppc64-gnu\r",
        "platform": "linux",
        "arch": "ppc64",
        "abi": "gnu\r"
      }
    ],
    "riscv64": [
      {
        "triple": "riscv64gc-unknown-linux-gnu\r",
        "platformArchABI": "linux-riscv64-gnu\r",
        "platform": "linux",
        "arch": "riscv64",
        "abi": "gnu\r"
      }
    ],
    "s390x": [
      {
        "triple": "s390x-unknown-linux-gnu\r",
        "platformArchABI": "linux-s390x-gnu\r",
        "platform": "linux",
        "arch": "s390x",
        "abi": "gnu\r"
      }
    ],
    "sparc64": [
      {
        "triple": "sparc64-unknown-linux-gnu\r",
        "platformArchABI": "linux-sparc64-gnu\r",
        "platform": "linux",
        "arch": "sparc64",
        "abi": "gnu\r"
      }
    ],
    "x64": [
      {
        "triple": "x86_64-unknown-linux-gnu\r",
        "platformArchABI": "linux-x64-gnu\r",
        "platform": "linux",
        "arch": "x64",
        "abi": "gnu\r"
      },
      {
        "triple": "x86_64-unknown-linux-gnux32\r",
        "platformArchABI": "linux-x64-gnux32\r",
        "platform": "linux",
        "arch": "x64",
        "abi": "gnux32\r"
      },
      {
        "triple": "x86_64-unknown-linux-musl\r",
        "platformArchABI": "linux-x64-musl\r",
        "platform": "linux",
        "arch": "x64",
        "abi": "musl\r"
      }
    ]
  }
}
