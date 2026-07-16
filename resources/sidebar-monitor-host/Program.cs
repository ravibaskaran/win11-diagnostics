// Story 15.1 — sidebar-monitor-host: elevated .NET sensor host.
//
// Loads LibreHardwareMonitorLib.dll directly (no HTTP server dependency),
// walks Computer.Hardware[].Sensors[], and emits JSON to stdout in the
// exact same shape as LHM's /data.json (Vec<LhmNode> tree) so the Rust
// adapter can consume it unchanged.
//
// The host runs elevated (spawned via ShellExecuteExW runas by the sidebar
// OhmSupervisor, wrapped in a Job Object for G10 ownership). It emits one
// JSON frame per second on stdout; the Rust side reads the latest line.
//
// Build: csc Program.cs -r:LibreHardwareMonitorLib.dll -out:sidebar-monitor-host.exe
//   (or via the .csproj + dotnet build if .NET SDK is available)
//
// Cited: Story 15.1, guardrails.md G10 (ownership) + G16 (pipe, not HTTP).
// License: MIT (same as the host workspace).

using System;
using System.Collections.Generic;
using System.Text.Json;
using LibreHardwareMonitor.Hardware;

namespace SidebarMonitorHost;

internal static class Program
{
    private static void Main(string[] args)
    {
        var computer = new Computer
        {
            IsCpuEnabled = true,
            IsGpuEnabled = true,
            IsMemoryEnabled = true,
            IsMotherboardEnabled = true,
            IsStorageEnabled = true,
            IsBatteryEnabled = true,
            IsNetworkEnabled = true,
            IsControllerEnabled = true,
        };
        computer.Open();

        // Signal readiness: emit one line "READY" so the Rust side knows
        // the library loaded + Open() succeeded (before the first frame).
        Console.Out.WriteLine("READY");
        Console.Out.Flush();

        var intervalMs = 1000; // default 1s
        if (args.Length > 0 && int.TryParse(args[0], out var parsed))
            intervalMs = Math.Clamp(parsed, 500, 10000);

        while (true)
        {
            var tree = BuildTree(computer);
            var json = JsonSerializer.Serialize(tree);
            Console.Out.WriteLine(json);
            Console.Out.Flush();
            System.Threading.Thread.Sleep(intervalMs);
        }
    }

    private static List<Dictionary<string, object?>> BuildTree(IComputer computer)
    {
        var nodes = new List<Dictionary<string, object?>>();
        foreach (IHardware hw in computer.Hardware)
        {
            hw.Update();
            var hwNode = new Dictionary<string, object?>
            {
                ["id"] = hw.Identifier.ToString(),
                ["text"] = hw.Name,
                ["type"] = "Node",
                ["children"] = BuildSensorChildren(hw),
                ["min"] = null as double?,
                ["max"] = null as double?,
                ["value"] = null as double?,
                ["imageindex"] = 0,
            };
            nodes.Add(hwNode);
        }
        return nodes;
    }

    private static List<Dictionary<string, object?>> BuildSensorChildren(IHardware hw)
    {
        var children = new List<Dictionary<string, object?>>();
        // Group sensors by SensorType → one folder node per type.
        var byType = new Dictionary<SensorType, List<ISensor>>();
        foreach (ISensor s in hw.Sensors)
        {
            if (!byType.ContainsKey(s.SensorType))
                byType[s.SensorType] = new List<ISensor>();
            byType[s.SensorType].Add(s);
        }
        foreach (var (st, sensors) in byType)
        {
            var typeName = st.ToString().ToLowerInvariant();
            var typeNode = new Dictionary<string, object?>
            {
                ["id"] = $"{hw.Identifier}{typeName}",
                ["text"] = typeName,
                ["type"] = "Node",
                ["children"] = new List<Dictionary<string, object?>>(),
                ["min"] = null as double?,
                ["max"] = null as double?,
                ["value"] = null as double?,
                ["imageindex"] = 0,
            };
            foreach (ISensor s in sensors)
            {
                var sensorNode = new Dictionary<string, object?>
                {
                    ["id"] = s.Identifier.ToString(),
                    ["text"] = s.Name,
                    ["type"] = "Sensor",
                    ["children"] = new List<Dictionary<string, object?>>(),
                    ["min"] = s.Min as double?,
                    ["max"] = s.Max as double?,
                    ["value"] = s.Value as double?,
                    ["imageindex"] = 0,
                };
                ((List<Dictionary<string, object?>>)typeNode["children"]!).Add(sensorNode);
            }
            children.Add(typeNode);
        }
        // Also include sub-hardware (e.g. motherboard → superIO).
        foreach (IHardware sub in hw.SubHardware)
        {
            sub.Update();
            children.Add(new Dictionary<string, object?>
            {
                ["id"] = sub.Identifier.ToString(),
                ["text"] = sub.Name,
                ["type"] = "Node",
                ["children"] = BuildSensorChildren(sub),
                ["min"] = null as double?,
                ["max"] = null as double?,
                ["value"] = null as double?,
                ["imageindex"] = 0,
            });
        }
        return children;
    }
}
