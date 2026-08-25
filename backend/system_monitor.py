"""
System Telemetry and Diagnostics Module for Project AETHER / CORTANA.
Provides real-time CPU, RAM, Disk, Network, Battery, and process monitoring
inspired by Omarchy system integration.
"""

import os
import platform
import time
import psutil
from datetime import datetime
from typing import Dict, Any, List

class SystemMonitor:
    def __init__(self):
        self.boot_time = psutil.boot_time()
        self.prev_net = psutil.net_io_counters()
        self.prev_net_time = time.time()

    def get_static_info(self) -> Dict[str, Any]:
        """System static hardware/OS information."""
        uname = platform.uname()
        cpu_freq = psutil.cpu_freq()
        mem = psutil.virtual_memory()
        
        # Detect Linux Distro if on Linux
        distro = "Linux"
        if os.path.exists("/etc/os-release"):
            try:
                with open("/etc/os-release") as f:
                    for line in f:
                        if line.startswith("PRETTY_NAME="):
                            distro = line.split("=", 1)[1].strip().strip('"')
                            break
            except Exception:
                pass

        return {
            "os": f"{uname.system} {uname.release}",
            "distro": distro,
            "architecture": uname.machine,
            "hostname": uname.node,
            "cpu_model": uname.processor or "Generic CPU",
            "cpu_cores_physical": psutil.cpu_count(logical=False) or 1,
            "cpu_cores_logical": psutil.cpu_count(logical=True) or 1,
            "cpu_max_freq_mhz": round(cpu_freq.max, 1) if cpu_freq else 0,
            "total_ram_gb": round(mem.total / (1024 ** 3), 2),
            "boot_time": datetime.fromtimestamp(self.boot_time).strftime("%Y-%m-%d %H:%M:%S"),
        }

    def get_telemetry(self) -> Dict[str, Any]:
        """Dynamic real-time telemetry snapshot."""
        now = time.time()
        uptime_seconds = int(now - self.boot_time)
        hours, remainder = divmod(uptime_seconds, 3600)
        minutes, seconds = divmod(remainder, 60)
        uptime_str = f"{hours:02d}h {minutes:02d}m {seconds:02d}s"

        # CPU per-core usage
        cpu_percent = psutil.cpu_percent(interval=None)
        cpu_per_core = psutil.cpu_percent(interval=None, percpu=True)
        cpu_freq = psutil.cpu_freq()

        # Memory usage
        mem = psutil.virtual_memory()
        swap = psutil.swap_memory()

        # Disk usage (root partition)
        try:
            disk = psutil.disk_usage("/")
            disk_info = {
                "total_gb": round(disk.total / (1024 ** 3), 1),
                "used_gb": round(disk.used / (1024 ** 3), 1),
                "free_gb": round(disk.free / (1024 ** 3), 1),
                "percent": disk.percent
            }
        except Exception:
            disk_info = {"total_gb": 0, "used_gb": 0, "free_gb": 0, "percent": 0}

        # Network speed calculation
        curr_net = psutil.net_io_counters()
        time_delta = max(now - self.prev_net_time, 0.001)
        bytes_sent_sec = (curr_net.bytes_sent - self.prev_net.bytes_sent) / time_delta
        bytes_recv_sec = (curr_net.bytes_recv - self.prev_net.bytes_recv) / time_delta
        
        self.prev_net = curr_net
        self.prev_net_time = now

        # Battery status
        battery_info = None
        try:
            battery = psutil.sensors_battery()
            if battery:
                battery_info = {
                    "percent": round(battery.percent, 1),
                    "power_plugged": battery.power_plugged,
                    "secsleft": battery.secsleft if battery.secsleft != psutil.POWER_TIME_UNLIMITED else None
                }
        except Exception:
            pass

        # Top processes by CPU/Memory
        top_processes = self._get_top_processes(limit=5)

        return {
            "timestamp": datetime.now().isoformat(),
            "uptime": uptime_str,
            "uptime_seconds": uptime_seconds,
            "cpu": {
                "total_percent": cpu_percent,
                "per_core": cpu_per_core,
                "current_freq_mhz": round(cpu_freq.current, 1) if cpu_freq else 0
            },
            "ram": {
                "used_gb": round(mem.used / (1024 ** 3), 2),
                "total_gb": round(mem.total / (1024 ** 3), 2),
                "available_gb": round(mem.available / (1024 ** 3), 2),
                "percent": mem.percent
            },
            "swap": {
                "used_gb": round(swap.used / (1024 ** 3), 2),
                "total_gb": round(swap.total / (1024 ** 3), 2),
                "percent": swap.percent
            },
            "disk": disk_info,
            "network": {
                "upload_kbps": round(bytes_sent_sec / 1024, 1),
                "download_kbps": round(bytes_recv_sec / 1024, 1),
                "total_sent_mb": round(curr_net.bytes_sent / (1024 ** 2), 1),
                "total_recv_mb": round(curr_net.bytes_recv / (1024 ** 2), 1)
            },
            "battery": battery_info,
            "top_processes": top_processes,
            "status": "NOMINAL" if cpu_percent < 85 and mem.percent < 90 else "HIGH_LOAD"
        }

    def _get_top_processes(self, limit: int = 5) -> List[Dict[str, Any]]:
        """Get top running processes by CPU usage."""
        procs = []
        for p in psutil.process_iter(['pid', 'name', 'cpu_percent', 'memory_percent']):
            try:
                info = p.info
                if info['name']:
                    procs.append({
                        "pid": info['pid'],
                        "name": info['name'][:20],
                        "cpu": round(info['cpu_percent'] or 0, 1),
                        "mem": round(info['memory_percent'] or 0, 1)
                    })
            except (psutil.NoSuchProcess, psutil.AccessDenied, psutil.ZombieProcess):
                pass
        
        # Sort by cpu desc
        procs.sort(key=lambda x: x['cpu'], reverse=True)
        return procs[:limit]

    def get_diagnostic_report(self) -> str:
        """Generate a concise Cortana-styled tactical diagnostic report."""
        telem = self.get_telemetry()
        static = self.get_static_info()
        
        report = (
            f"SYSTEM DIAGNOSTIC REPORT // {static['distro'].upper()} [{static['architecture']}]\n"
            f"━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n"
            f"Status: {telem['status']} | Uptime: {telem['uptime']}\n"
            f"CPU Load: {telem['cpu']['total_percent']}% across {static['cpu_cores_logical']} cores ({telem['cpu']['current_freq_mhz']} MHz)\n"
            f"RAM Usage: {telem['ram']['used_gb']} GB / {telem['ram']['total_gb']} GB ({telem['ram']['percent']}%)\n"
            f"Storage: {telem['disk']['used_gb']} GB / {telem['disk']['total_gb']} GB ({telem['disk']['percent']}% used)\n"
            f"Network I/O: ↓ {telem['network']['download_kbps']} KB/s | ↑ {telem['network']['upload_kbps']} KB/s\n"
        )
        if telem['battery']:
            plugged = "AC Connected" if telem['battery']['power_plugged'] else "On Battery"
            report += f"Power: {telem['battery']['percent']}% [{plugged}]\n"
        
        report += "Top Processes: " + ", ".join([f"{p['name']} ({p['cpu']}%)" for p in telem['top_processes'][:3]])
        return report

# Global instance
system_monitor = SystemMonitor()
