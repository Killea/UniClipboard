//go:build !linux

package main

import "os/exec"

// startHostHelper starts a host program. Only the Linux AppImage changes the environment of its helpers (host_helper_env_linux.go).
func startHostHelper(name string, args ...string) error {
	cmd := exec.Command(name, args...)
	if err := cmd.Start(); err != nil {
		return err
	}
	go func() { _ = cmd.Wait() }()
	return nil
}

// runHostHelperSync runs a host program to completion and returns its trimmed stdout. Outside the Linux
// AppImage there is no environment to clean. Exit status 1 is reported as an empty result with a nil
// error (a cancelled file dialog); any other failure is returned.
func runHostHelperSync(name string, args ...string) (string, error) {
	out, err := exec.Command(name, args...).Output()
	return decodeHostHelperOutput(out, err)
}
