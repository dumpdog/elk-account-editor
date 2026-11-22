namespace Elk.Protocol;

/// <summary>One area's live state (from the panel's status query 6).</summary>
public sealed record AreaLive(int Area, bool Ready, bool ReadyToForceArm, int Armed, string ArmedName, bool Alarm);

/// <summary>One zone's live state (status queries 8 and 16).</summary>
public sealed record ZoneLive(int Zone, string State, bool Bypassed, bool ForceArmed, string Physical, double Volts);

public sealed record LiveSnapshot(List<AreaLive> Areas, List<ZoneLive> Zones, List<string> System);

/// <summary>
/// The live status ElkRP shows in "Current Status and Remote Control". All four queries are read only: <c>7F 00 00 n</c> with n = 6 (areas),
/// 7 (system alarms and troubles), 8 (zones) and 16 (zone voltages). The decoding follows ElkRP's own RemoteControl form:
/// area n state byte = reply[8n]: bits 0-1 = 3 alarm, bits 2-4 = 0 not ready, 4 ready, 8 ready to force arm, bits 5-7 = armed level (0 disarmed, 1 away, 2 stay,
/// 3 stay instant, 4 night, 5 night instant, 6 vacation); zone n state byte = reply[n+3]: bits 0-1 = physical (0 N/A, 1 open, 2 EOL resistor, 3 short),
/// bits 2-3 = logical state (0 normal, 1 trouble, 2/3 violated), 0x20 bypassed, 0x10 force armed; voltage = reply[n+3] / 10; system alarms and troubles =
/// every non-zero byte of reply[4..55] names an entry of <see cref="SystemNames"/>.
/// </summary>
public static class PanelLive
{
    public static readonly string[] ArmedNames = { "Disarmed", "Armed Away", "Armed Stay", "Armed Stay Instant", "Armed Night", "Armed Night Instant", "Armed Vacation" };
    private static readonly string[] LogicalNames = { "Normal", "Trouble", "Violated", "Violated" };
    private static readonly string[] PhysicalNames = { "N/A", "Open (+)", "EOLR (=)", "Short (-)" };

    public static readonly string[] SystemNames =
    {
        "", "", "Fire Alarm", "Medical Trouble/Alarm", "Police Trouble/Alarm", "Burglar Trouble/Alarm", "Aux 1 Trouble/Alarm", "Aux 2 Trouble/Alarm", "", "",
        "Carbon Monoxide Trouble/Alarm", "Emergency Trouble/Alarm", "Freeze Trouble/Alarm", "Gas Trouble/Alarm", "Heat Trouble/Alarm", "Water Trouble/Alarm",
        "Fire Supervisory Alarm", "Fire 'Verified' Alarm", "A.C. Failure", "Box Tamper", "Dialer Communication Failure", "EEPROM Memory Error", "Control Low Battery",
        "Transmitter Low Battery", "Control Over Current", "Telephone Fault", "", "Output 2 Trouble", "Keypad Trouble", "Input Expander Trouble",
        "Output Expander Trouble", "Audio Network Controller Trouble", "", "", "", "Flash Memory Error", "Security (Day) Alert", "Serial Port Expander Trouble",
        "Lost Transmitter Trouble", "Smoke Detector Needs Cleaning", "Ethernet Expander Trouble", "", "", "", "", "", "", "", "", "", "", "Fire Trouble",
    };

    private static async Task<byte[]> Query(ElkSession s, byte n, int minLength, CancellationToken ct)
    {
        await s.SendAsync(new byte[] { 0x7F, 0, 0, n }, ct);
        var r = await s.ReceiveAsync(0x7F, ct);
        if (r.Length < minLength || r[3] != n) throw new InvalidDataException($"unexpected reply to status query {n}: {r.Length} bytes");
        return r;
    }

    public static List<AreaLive> DecodeAreas(byte[] r)
    {
        var list = new List<AreaLive>();
        for (int a = 1; a <= 8 && 8 * a < r.Length; a++)
        {
            int b = r[8 * a], ready = b & 0x1C, armed = (b >> 5) & 7;
            list.Add(new AreaLive(a, ready != 0, ready == 8, armed, armed < ArmedNames.Length ? ArmedNames[armed] : "Armed (" + armed + ")", (b & 3) == 3));
        }
        return list;
    }

    public static List<string> DecodeSystem(byte[] r)
    {
        var list = new List<string>();
        for (int i = 4; i <= 55 && i < r.Length; i++)
            if (r[i] != 0) list.Add(i - 4 < SystemNames.Length && SystemNames[i - 4] != "" ? SystemNames[i - 4] : "Status item " + (i - 4));
        return list;
    }

    public static List<ZoneLive> DecodeZones(byte[] status, byte[]? volts)
    {
        var list = new List<ZoneLive>();
        for (int z = 1; z <= 208 && z + 3 < status.Length; z++)
        {
            int b = status[z + 3];
            list.Add(new ZoneLive(z, LogicalNames[(b >> 2) & 3], (b & 0x20) != 0, (b & 0x10) != 0, PhysicalNames[b & 3], volts != null && z + 3 < volts.Length ? volts[z + 3] / 10.0 : 0));
        }
        return list;
    }

    /// <summary>Reads the current status of the whole panel (four round trips).</summary>
    public static async Task<LiveSnapshot> ReadAsync(ElkSession s, CancellationToken ct = default)
    {
        var areas = await Query(s, 6, 68, ct);
        var system = await Query(s, 7, 56, ct);
        var zones = await Query(s, 8, 212, ct);
        var volts = await Query(s, 16, 212, ct);
        return new LiveSnapshot(DecodeAreas(areas), DecodeZones(zones, volts), DecodeSystem(system));
    }
}
