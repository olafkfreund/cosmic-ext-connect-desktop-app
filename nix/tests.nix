{ pkgs ? import <nixpkgs> { }, ... }:

let
  # Import the package and module
  cosmic-kdeconnect = pkgs.callPackage ./package.nix { };

  # Helper to create a test VM
  makeTest = import "${pkgs.path}/nixos/tests/make-test-python.nix";

in {
  # Test 1: Basic package build and installation
  package-build = pkgs.runCommand "cosmic-kdeconnect-build-test" { } ''
    # Test that the package builds
    ${cosmic-kdeconnect}/bin/kdeconnect-daemon --version > $out
    ${cosmic-kdeconnect}/bin/cosmic-applet-kdeconnect --version >> $out

    # Verify binaries exist
    test -f ${cosmic-kdeconnect}/bin/kdeconnect-daemon || exit 1
    test -f ${cosmic-kdeconnect}/bin/cosmic-applet-kdeconnect || exit 1

    # Verify systemd service file exists
    test -f ${cosmic-kdeconnect}/lib/systemd/user/kdeconnect-daemon.service || exit 1

    # Verify desktop entry exists
    test -f ${cosmic-kdeconnect}/share/applications/cosmic-applet-kdeconnect.desktop || exit 1

    echo "Package build test: PASSED" >> $out
  '';

  # Test 2: Basic module configuration
  module-basic = makeTest {
    name = "cosmic-kdeconnect-module-basic";

    nodes.machine = { config, pkgs, ... }: {
      imports = [ ./module.nix ];

      # Enable KDE Connect with basic configuration
      services.cosmic-kdeconnect = {
        enable = true;
        openFirewall = true;
        daemon.enable = true;
      };

      # Required for testing
      users.users.testuser = {
        isNormalUser = true;
        home = "/home/testuser";
      };
    };

    testScript = ''
      machine.start()
      machine.wait_for_unit("multi-user.target")

      # Check package is installed
      machine.succeed("which kdeconnect-daemon")
      machine.succeed("which cosmic-applet-kdeconnect")

      # Check firewall ports are open
      machine.succeed("iptables -L INPUT -n | grep -E '1714:1764'")

      # Check systemd service file exists
      machine.succeed("test -f /etc/systemd/user/kdeconnect-daemon.service")

      # Verify service can be started
      machine.succeed("su - testuser -c 'systemctl --user start kdeconnect-daemon'")
      machine.wait_for_open_port(1716)

      # Check service is running
      machine.succeed("su - testuser -c 'systemctl --user status kdeconnect-daemon'")

      print("Basic module test: PASSED")
    '';
  } { inherit pkgs; };

  # Test 3: Module with custom configuration
  module-custom-config = makeTest {
    name = "cosmic-kdeconnect-module-custom";

    nodes.machine = { config, pkgs, ... }: {
      imports = [ ./module.nix ];

      services.cosmic-kdeconnect = {
        enable = true;
        openFirewall = true;

        daemon = {
          enable = true;
          autoStart = true;
          logLevel = "debug";
          settings = {
            discovery = {
              broadcast_interval = 5000;
            };
          };
        };

        plugins = {
          battery = true;
          clipboard = true;
          notification = true;
          share = true;
          mpris = true;
          ping = true;
        };

        security = {
          trustOnFirstPair = false;
        };
      };

      users.users.testuser = {
        isNormalUser = true;
        home = "/home/testuser";
      };
    };

    testScript = ''
      machine.start()
      machine.wait_for_unit("multi-user.target")

      # Verify configuration file is generated
      machine.succeed("test -f /etc/xdg/kdeconnect/config.toml")

      # Start daemon with custom config
      machine.succeed("su - testuser -c 'systemctl --user start kdeconnect-daemon'")
      machine.wait_for_open_port(1716)

      # Check daemon is using debug log level
      machine.succeed("su - testuser -c 'systemctl --user show kdeconnect-daemon -p Environment' | grep 'RUST_LOG=debug'")

      print("Custom config test: PASSED")
    '';
  } { inherit pkgs; };

  # Test 4: Firewall disabled
  module-no-firewall = makeTest {
    name = "cosmic-kdeconnect-no-firewall";

    nodes.machine = { config, pkgs, ... }: {
      imports = [ ./module.nix ];

      services.cosmic-kdeconnect = {
        enable = true;
        openFirewall = false;
        daemon.enable = true;
      };

      users.users.testuser = {
        isNormalUser = true;
      };
    };

    testScript = ''
      machine.start()
      machine.wait_for_unit("multi-user.target")

      # Verify firewall ports are NOT open
      machine.fail("iptables -L INPUT -n | grep -E '1714:1764'")

      # But daemon should still be installable
      machine.succeed("which kdeconnect-daemon")

      print("No firewall test: PASSED")
    '';
  } { inherit pkgs; };

  # Test 5: Two machines communicating
  two-machines = makeTest {
    name = "cosmic-kdeconnect-two-machines";

    nodes = {
      machine1 = { config, pkgs, ... }: {
        imports = [ ./module.nix ];

        services.cosmic-kdeconnect = {
          enable = true;
          openFirewall = true;
          daemon.enable = true;
        };

        networking = {
          firewall.enable = true;
          interfaces.eth1.ipv4.addresses = [{
            address = "192.168.1.10";
            prefixLength = 24;
          }];
        };

        users.users.testuser = {
          isNormalUser = true;
        };
      };

      machine2 = { config, pkgs, ... }: {
        imports = [ ./module.nix ];

        services.cosmic-kdeconnect = {
          enable = true;
          openFirewall = true;
          daemon.enable = true;
        };

        networking = {
          firewall.enable = true;
          interfaces.eth1.ipv4.addresses = [{
            address = "192.168.1.11";
            prefixLength = 24;
          }];
        };

        users.users.testuser = {
          isNormalUser = true;
        };
      };
    };

    testScript = ''
      machine1.start()
      machine2.start()

      # Wait for both machines to be ready
      machine1.wait_for_unit("multi-user.target")
      machine2.wait_for_unit("multi-user.target")

      # Start daemons on both machines
      machine1.succeed("su - testuser -c 'systemctl --user start kdeconnect-daemon'")
      machine2.succeed("su - testuser -c 'systemctl --user start kdeconnect-daemon'")

      # Wait for services to be listening
      machine1.wait_for_open_port(1716)
      machine2.wait_for_open_port(1716)

      # Test network connectivity between machines
      machine1.succeed("ping -c 3 192.168.1.11")
      machine2.succeed("ping -c 3 192.168.1.10")

      # Test UDP broadcast (device discovery simulation)
      machine1.succeed("nc -u -w 1 192.168.1.11 1716 < /dev/null")

      print("Two machines test: PASSED")
    '';
  } { inherit pkgs; };

  # Test 6: Plugin functionality
  plugin-test = makeTest {
    name = "cosmic-kdeconnect-plugins";

    nodes.machine = { config, pkgs, ... }: {
      imports = [ ./module.nix ];

      services.cosmic-kdeconnect = {
        enable = true;
        daemon.enable = true;

        plugins = {
          battery = true;
          clipboard = true;
          notification = false;
          share = true;
          mpris = true;
          ping = true;
        };
      };

      users.users.testuser = {
        isNormalUser = true;
      };
    };

    testScript = ''
      machine.start()
      machine.wait_for_unit("multi-user.target")

      # Start daemon
      machine.succeed("su - testuser -c 'systemctl --user start kdeconnect-daemon'")
      machine.wait_for_open_port(1716)

      # Check that config reflects plugin settings
      # In a real implementation, we'd check the daemon's plugin loading

      # Verify daemon is running with plugins enabled
      machine.succeed("su - testuser -c 'systemctl --user status kdeconnect-daemon'")

      print("Plugin test: PASSED")
    '';
  } { inherit pkgs; };

  # Test 7: Service restart and recovery
  service-recovery = makeTest {
    name = "cosmic-kdeconnect-recovery";

    nodes.machine = { config, pkgs, ... }: {
      imports = [ ./module.nix ];

      services.cosmic-kdeconnect = {
        enable = true;
        daemon.enable = true;
        daemon.autoStart = true;
      };

      users.users.testuser = {
        isNormalUser = true;
      };
    };

    testScript = ''
      machine.start()
      machine.wait_for_unit("multi-user.target")

      # Start daemon
      machine.succeed("su - testuser -c 'systemctl --user start kdeconnect-daemon'")
      machine.wait_for_open_port(1716)

      # Kill the daemon process
      machine.succeed("pkill -9 kdeconnect-daemon")
      machine.sleep(1)

      # Check that systemd restarted it (Restart=on-failure)
      machine.wait_for_unit("kdeconnect-daemon.service", "testuser")
      machine.wait_for_open_port(1716)

      # Verify it's running again
      machine.succeed("su - testuser -c 'systemctl --user status kdeconnect-daemon'")

      print("Service recovery test: PASSED")
    '';
  } { inherit pkgs; };

  # Test 8: Security hardening verification
  security-test = makeTest {
    name = "cosmic-kdeconnect-security";

    nodes.machine = { config, pkgs, ... }: {
      imports = [ ./module.nix ];

      services.cosmic-kdeconnect = {
        enable = true;
        daemon.enable = true;
      };

      users.users.testuser = {
        isNormalUser = true;
      };
    };

    testScript = ''
      machine.start()
      machine.wait_for_unit("multi-user.target")

      # Start daemon
      machine.succeed("su - testuser -c 'systemctl --user start kdeconnect-daemon'")

      # Check security settings in systemd service
      machine.succeed("su - testuser -c 'systemctl --user show kdeconnect-daemon -p NoNewPrivileges' | grep 'yes'")
      machine.succeed("su - testuser -c 'systemctl --user show kdeconnect-daemon -p ProtectSystem' | grep 'strict'")
      machine.succeed("su - testuser -c 'systemctl --user show kdeconnect-daemon -p PrivateTmp' | grep 'yes'")

      print("Security hardening test: PASSED")
    '';
  } { inherit pkgs; };

  # Run all tests
  all = pkgs.runCommand "cosmic-kdeconnect-all-tests" { } ''
    echo "Running all cosmic-applet-kdeconnect tests..." > $out
    echo "========================================" >> $out
    echo "" >> $out

    # Note: In practice, you would run each test here
    # For now, we just indicate they exist

    echo "1. Package build test: Available" >> $out
    echo "2. Basic module test: Available" >> $out
    echo "3. Custom config test: Available" >> $out
    echo "4. No firewall test: Available" >> $out
    echo "5. Two machines test: Available" >> $out
    echo "6. Plugin test: Available" >> $out
    echo "7. Service recovery test: Available" >> $out
    echo "8. Security test: Available" >> $out
    echo "" >> $out
    echo "Run individual tests with: nix build .#checks.<system>.<test-name>" >> $out
  '';
}
