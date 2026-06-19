import { invoke } from '@tauri-apps/api/core';
import type { Target, DashboardData, HopStats, TimeRange, LoadTestResult, ProbeMode } from '@netmon/shared';

export async function getTargets(): Promise<Target[]> {
  return invoke('get_targets');
}

export async function addTarget(address: string, label: string, probeMode?: ProbeMode): Promise<Target> {
  return invoke('add_target', { address, label, probeMode });
}

export async function setProbeMode(address: string, probeMode: ProbeMode): Promise<void> {
  return invoke('set_probe_mode', { address, probeMode });
}

export async function removeTarget(id: number): Promise<void> {
  return invoke('remove_target', { id });
}

export async function getDashboard(target: string, range: TimeRange): Promise<DashboardData> {
  // Tauri invoke serializes the args as JSON — range can be "1h" or { customDay: 123 }
  return invoke('get_dashboard', { target, range });
}

export async function getLiveStats(target: string): Promise<HopStats[]> {
  return invoke('get_live_stats', { target });
}

export async function pauseMonitoring(): Promise<void> {
  return invoke('pause_monitoring');
}

export async function resumeMonitoring(): Promise<void> {
  return invoke('resume_monitoring');
}

export async function getMonitoringPaused(): Promise<boolean> {
  return invoke('get_monitoring_paused');
}

export async function runLoadTest(): Promise<LoadTestResult> {
  return invoke('run_load_test');
}

export async function getLoadTestHistory(): Promise<LoadTestResult[]> {
  return invoke('get_load_test_history');
}
