{ config, lib, pkgs, modulesPath, signer, ... }:
let
  kernel = config.boot.kernelPackages.kernel;
in
{
  imports = [
    (modulesPath + "/profiles/minimal.nix")
    (modulesPath + "/installer/sd-card/sd-image.nix")
  ];
  system.stateVersion = "26.05";
  networking.hostName = "signer-pi";
  nixpkgs.config.allowUnfree = true;
  boot.kernelPackages = pkgs.linuxKernel.packages.linux_rpi3;
  boot.loader.grub.enable = false;
  boot.loader.generic-extlinux-compatible.enable = true;
  boot.initrd.availableKernelModules = [ "mmc_block" "sdhci_iproc" "bcm2835" ];
  boot.initrd.kernelModules = [ "spi-bcm2835" "fb_st7789v" ];
  boot.initrd.extraUtilsCommands = ''
    cp ${signer}/bin/signer-boot-status $out/bin/
  '';
  boot.initrd.preDeviceCommands = ''
    echo devices > /run/signer-boot-phase
    signer-boot-status </dev/null >/dev/null 2>&1 &
  '';
  boot.initrd.postDeviceCommands = "echo mount > /run/signer-boot-phase";
  boot.initrd.preFailCommands = "echo failed > /run/signer-boot-phase";
  boot.postBootCommands = lib.mkMerge [
    (lib.mkBefore ''
      if [ -f /nix-path-registration ]; then
        echo first > /run/signer-boot-phase
      fi
    '')
    (lib.mkAfter "echo services > /run/signer-boot-phase")
  ];
  boot.initrd.allowMissingModules = true;
  boot.kernelModules = [ "i2c-dev" "fb_st7789v" "brcmfmac" ];
  hardware.enableAllHardware = lib.mkForce false;
  hardware.firmware = [ pkgs.raspberrypiWirelessFirmware pkgs.wireless-regdb ];
  hardware.bluetooth = { enable = true; powerOnBoot = true; };
  networking.useDHCP = false;
  networking.useNetworkd = true;
  networking.usePredictableInterfaceNames = false;
  systemd.network.networks."10-panel" = {
    matchConfig.Name = "wlan0";
    address = [ "10.42.0.1/24" ];
    networkConfig = { DHCPServer = true; ConfigureWithoutCarrier = true; LinkLocalAddressing = "no"; };
    dhcpServerConfig = { EmitRouter = false; EmitDNS = false; PoolOffset = 10; PoolSize = 32; };
  };
  services.hostapd = {
    enable = true;
    radios.wlan0 = {
      band = "2g"; channel = 6; countryCode = "PT";
      wifi4.enable = false;
      wifi5.enable = false;
      networks.wlan0 = {
        ssid = "Signer-Pi";
        authentication = { mode = "wpa2-sha1"; wpaPasswordFile = "/var/lib/signer-appliance/wifi-passphrase"; };
      };
    };
  };
  networking.firewall.interfaces.wlan0 = { allowedTCPPorts = [ 22 ]; allowedUDPPorts = [ 67 ]; };
  services.openssh = { enable = true; openFirewall = false; settings = { PasswordAuthentication = false; KbdInteractiveAuthentication = false; PermitRootLogin = "prohibit-password"; }; };
  systemd.services.signer-pi = {
    description = "Cardano signer with physical panel approval";
    wantedBy = [ "multi-user.target" ];
    after = [ "systemd-networkd.service" ];
    path = [ pkgs.util-linux ];
    serviceConfig = {
      ExecStart = "${signer}/bin/signer-pi --bluetooth";
      Restart = "always"; RestartSec = 3;
      StateDirectory = "signer-pi"; StateDirectoryMode = "0700";
    };
    unitConfig.StartLimitIntervalSec = 0;
  };
  sdImage = {
    firmwareSize = 128;
    firmwarePartitionName = "SIGNERBOOT";
    rootVolumeLabel = "SIGNERROOT";
    populateRootCommands = ''
      mkdir -p files/var/lib/signer-appliance
      cat > files/signer-init <<'INIT'
      #!${pkgs.runtimeShell}
      if [ -f /nix-path-registration ]; then
        : > /run/signer-first-boot
      fi
      echo activation > /run/signer-boot-phase
      ${signer}/bin/signer-boot-status </dev/null >/dev/null 2>&1 &
      exec ${config.system.build.toplevel}/init
      INIT
      chmod 755 files/signer-init
    '';
    populateFirmwareCommands = ''
      mkdir -p firmware/overlays
      cp ${pkgs.raspberrypifw}/share/raspberrypi/boot/{bootcode.bin,start.elf,fixup.dat} firmware/
      cp ${kernel}/dtbs/broadcom/bcm2710-rpi-3-b.dtb firmware/
      cp ${kernel}/dtbs/overlays/minipitft13.dtbo firmware/overlays/
      cp ${kernel}/dtbs/overlays/overlay_map.dtb firmware/overlays/ || true
      cp ${kernel}/Image firmware/kernel8.img
      cp ${config.system.build.initialRamdisk}/${config.system.boot.loader.initrdFile} firmware/initrd
      cat > firmware/config.txt <<'CONFIG'
      [all]
      arm_64bit=1
      kernel=kernel8.img
      initramfs initrd followkernel
      dtparam=krnbt=on
      dtparam=i2c_arm=on
      dtparam=spi=on
      dtoverlay=minipitft13,rotate=0
      CONFIG
      echo 'init=/signer-init root=LABEL=SIGNERROOT rootfstype=ext4 rootwait ${lib.concatStringsSep " " config.boot.kernelParams}' > firmware/cmdline.txt
    '';
  };
}
