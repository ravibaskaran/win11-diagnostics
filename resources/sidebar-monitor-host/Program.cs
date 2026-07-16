// Story 15.1 — sidebar-monitor-host: elevated .NET sensor host.
// Compatible with csc.exe v4.0.30319 (C# 5.0 / .NET Framework 4.x).
//
// Loads LibreHardwareMonitorLib.dll directly (no HTTP server dependency),
// walks Computer.Hardware[].Sensors[], and emits JSON to stdout in the
// same shape as LHM's /data.json tree so the Rust adapter consumes it
// unchanged.
//
// Cited: Story 15.1, guardrails.md G10 (ownership) + G16 (pipe, not HTTP).
// License: MIT.

using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using LibreHardwareMonitor.Hardware;

namespace SidebarMonitorHost
{
    internal static class Program
    {
        private static void Main(string[] args)
        {
            Computer computer = new Computer();
            computer.IsCpuEnabled = true;
            computer.IsGpuEnabled = true;
            computer.IsMemoryEnabled = true;
            computer.IsMotherboardEnabled = true;
            computer.IsStorageEnabled = true;
            computer.IsBatteryEnabled = true;
            computer.IsNetworkEnabled = true;
            computer.IsControllerEnabled = true;
            computer.Open();

            // Signal readiness so the Rust side knows the library loaded.
            Console.Out.WriteLine("READY");
            Console.Out.Flush();

            int intervalMs = 1000;
            if (args.Length > 0)
            {
                int parsed;
                if (int.TryParse(args[0], out parsed))
                {
                    if (parsed < 500) parsed = 500;
                    if (parsed > 10000) parsed = 10000;
                    intervalMs = parsed;
                }
            }

            while (true)
            {
                string json = BuildJson(computer);
                Console.Out.WriteLine(json);
                Console.Out.Flush();
                System.Threading.Thread.Sleep(intervalMs);
            }
        }

        // Hand-rolled JSON serializer (avoids System.Text.Json dependency
        // which requires .NET 5+ or NuGet package on Framework 4.x).
        private static string BuildJson(IComputer computer)
        {
            StringBuilder sb = new StringBuilder();
            sb.Append("[");
            bool first = true;
            foreach (IHardware hw in computer.Hardware)
            {
                hw.Update();
                if (!first) sb.Append(",");
                first = false;
                AppendNode(sb, hw.Identifier.ToString(), hw.Name, BuildSensorChildren(hw));
            }
            sb.Append("]");
            return sb.ToString();
        }

        private static string BuildSensorChildren(IHardware hw)
        {
            StringBuilder sb = new StringBuilder();
            sb.Append("[");
            bool first = true;

            // Group sensors by SensorType — one folder node per type.
            Dictionary<SensorType, List<ISensor>> byType = new Dictionary<SensorType, List<ISensor>>();
            foreach (ISensor s in hw.Sensors)
            {
                if (!byType.ContainsKey(s.SensorType))
                    byType[s.SensorType] = new List<ISensor>();
                byType[s.SensorType].Add(s);
            }
            foreach (KeyValuePair<SensorType, List<ISensor>> entry in byType)
            {
                SensorType st = entry.Key;
                List<ISensor> sensors = entry.Value;
                string typeName = st.ToString().ToLowerInvariant();
                string folderId = hw.Identifier.ToString() + typeName;

                if (!first) sb.Append(",");
                first = false;
                sb.Append("{");
                sb.Append("\"id\":\"").Append(EscapeJson(folderId)).Append("\",");
                sb.Append("\"text\":\"").Append(EscapeJson(typeName)).Append("\",");
                sb.Append("\"type\":\"Node\",");
                sb.Append("\"children\":[");
                bool firstSensor = true;
                foreach (ISensor s in sensors)
                {
                    if (!firstSensor) sb.Append(",");
                    firstSensor = false;
                    sb.Append("{");
                    sb.Append("\"id\":\"").Append(EscapeJson(s.Identifier.ToString())).Append("\",");
                    sb.Append("\"text\":\"").Append(EscapeJson(s.Name)).Append("\",");
                    sb.Append("\"type\":\"Sensor\",");
                    sb.Append("\"children\":[],");
                    sb.Append("\"min\":").Append(FormatDouble(s.Min)).Append(",");
                    sb.Append("\"max\":").Append(FormatDouble(s.Max)).Append(",");
                    sb.Append("\"value\":").Append(FormatDouble(s.Value)).Append(",");
                    sb.Append("\"imageindex\":0");
                    sb.Append("}");
                }
                sb.Append("],");
                sb.Append("\"min\":null,\"max\":null,\"value\":null,\"imageindex\":0");
                sb.Append("}");
            }
            // Sub-hardware (e.g. motherboard -> superIO).
            foreach (IHardware sub in hw.SubHardware)
            {
                sub.Update();
                if (!first) sb.Append(",");
                first = false;
                AppendNode(sb, sub.Identifier.ToString(), sub.Name, BuildSensorChildren(sub));
            }
            sb.Append("]");
            return sb.ToString();
        }

        private static void AppendNode(StringBuilder sb, string id, string text, string childrenJson)
        {
            sb.Append("{");
            sb.Append("\"id\":\"").Append(EscapeJson(id)).Append("\",");
            sb.Append("\"text\":\"").Append(EscapeJson(text)).Append("\",");
            sb.Append("\"type\":\"Node\",");
            sb.Append("\"children\":").Append(childrenJson).Append(",");
            sb.Append("\"min\":null,\"max\":null,\"value\":null,\"imageindex\":0");
            sb.Append("}");
        }

        private static string FormatDouble(double? value)
        {
            if (!value.HasValue) return "null";
            double d = value.Value;
            if (double.IsNaN(d) || double.IsInfinity(d)) return "null";
            return d.ToString("R", CultureInfo.InvariantCulture);
        }

        private static string EscapeJson(string s)
        {
            if (s == null) return "";
            return s.Replace("\\", "\\\\").Replace("\"", "\\\"");
        }
    }
}
