using System.Globalization;
using System.Text;

namespace Elk.Protocol;

/// <summary>Default zone voice description, as built by ElkRP (Zone_Definitions.cs, around line 8000) and checked against
/// the stored voice words of 912 default zones (1-16 and 161-208): "Zone", then for 100-299 "One"/"Two" (plus "Hundred" if the
/// remainder is under 10), then the teens word or the tens word, then the ones digit when the remainder is under 10 or over 20.
/// Word ids come from Controls2.mdb WordLists for M1G (M1 Std has the same ids). ElkRP spells 40 "Fourty"; id 43.
/// Not verified for EZ8 panels (their template accounts stored different voices).</summary>
public static class ZoneVoice
{
    private const ushort Zone = 471, Hundred = 49;
    private static readonly ushort[] Ones = { 0, 22, 23, 24, 25, 26, 27, 28, 29, 30 };           // One..Nine
    private static readonly ushort[] Teens = { 31, 32, 33, 34, 35, 36, 37, 38, 39, 40 };        // Ten..Nineteen
    private static readonly ushort[] Tens = { 0, 0, 41, 42, 43, 44, 45, 46, 47, 48 };           // Twenty..Ninety

    public static ushort[] Words(int zone)
    {
        if (zone is < 1 or > 208) throw new ArgumentOutOfRangeException(nameof(zone));
        var w = new List<ushort> { Zone };
        int r = zone % 100;
        if (zone / 100 is 1 or 2) { w.Add(Ones[zone / 100]); if (r < 10) w.Add(Hundred); }
        if (r is >= 10 and <= 19) w.Add(Teens[r - 10]);
        else if (r / 10 >= 2) w.Add(Tens[r / 10]);
        if ((r < 10 || r > 20) && r % 10 != 0) w.Add(Ones[r % 10]);
        while (w.Count < 6) w.Add(0);
        return w.ToArray();
    }

    public static void Apply(ZoneRecord z, int zone)
    {
        var w = Words(zone);
        for (int i = 0; i < 6; i++) { z.Bytes[20 + 2 * i] = (byte)(w[i] >> 8); z.Bytes[21 + 2 * i] = (byte)(w[i] & 0xFF); }
    }
}

/// <summary>One row of the import file: zone, name, definition, type, area, attributes.</summary>
public sealed record ZoneSpec(int Zone, string Name, int Definition, int Type, int Area, ZoneFlag1 Flags1, bool SwingerShutdown, bool SilentAlarm)
{
    public ZoneRecord ToRecord()
    {
        var z = new ZoneRecord { Function = (byte)Definition, Flag1 = (byte)Flags1, Name = Name };
        byte f2 = ZoneFlag2.With(0, type: Type, area: Area);
        if (SwingerShutdown) f2 |= ZoneFlag2.SwingerShutdown;
        if (SilentAlarm) f2 |= ZoneFlag2.SilentAlarm;
        z.Flag2 = f2;
        ZoneVoice.Apply(z, Zone);
        return z;
    }
}

/// <summary>CSV: Zone,Name,Definition,Type,Area,Attributes. Attributes are separated by | ; or spaces-and-commas inside quotes.
/// Accepted names (case, spaces and punctuation ignored): SilentAlarm, UseDialerDelay, ListenIn, Bypassable, ForceArmable,
/// SwingerShutdown, PeriodicTrip, FastLoopResponse, EnableChime, InCrossZonePool.</summary>
public static class ZoneCsv
{
    public const string Header = "Zone,Name,Definition,Type,Area,Attributes";

    private static readonly Dictionary<string, ZoneFlag1> Flag1Names = new()
    {
        ["bypassable"] = ZoneFlag1.Bypassable, ["periodictrip"] = ZoneFlag1.PeriodicTrip, ["fastloopresponse"] = ZoneFlag1.FastResponse,
        ["forcearmable"] = ZoneFlag1.ForceArm, ["enablechime"] = ZoneFlag1.Chime, ["incrosszonepool"] = ZoneFlag1.CrossZoned,
        ["usedialerdelay"] = ZoneFlag1.UseDialerDelay, ["listenin"] = ZoneFlag1.ListenIn,
    };

    public static (List<ZoneSpec> Zones, List<string> Errors) Parse(string text)
    {
        var zones = new List<ZoneSpec>(); var errors = new List<string>(); var seen = new HashSet<int>();
        var lines = text.Split('\n').Select(l => l.TrimEnd('\r')).ToList();
        for (int i = 0; i < lines.Count; i++)
        {
            var line = lines[i];
            if (string.IsNullOrWhiteSpace(line) || line.StartsWith('#')) continue;
            var f = SplitCsv(line);
            if (i == 0 && !int.TryParse(f[0], out _)) continue;   // header
            try
            {
                if (f.Count < 5) throw new FormatException("need Zone,Name,Definition,Type,Area[,Attributes]");
                int zone = int.Parse(f[0], CultureInfo.InvariantCulture), def = int.Parse(f[2]), type = int.Parse(f[3]), area = int.Parse(f[4]);
                if (zone is < 1 or > 208) throw new FormatException($"zone {zone} out of range 1-208");
                if (!seen.Add(zone)) throw new FormatException($"zone {zone} listed twice");
                if (f[1].Length > 16 || f[1].Any(c => c > 126 || c < 32)) throw new FormatException("name must be at most 16 printable ASCII characters");
                if (def is < 0 or > 255) throw new FormatException("definition must be 0-255");
                if (type is < 0 or > 7) throw new FormatException("type must be 0-7");
                if (area is < 1 or > 8) throw new FormatException("area must be 1-8");
                ZoneFlag1 flags = 0; bool swinger = false, silent = false;
                foreach (var a in (f.Count > 5 ? f[5] : "").Split(new[] { '|', ';', ',' }, StringSplitOptions.RemoveEmptyEntries))
                {
                    var key = new string(a.Where(char.IsLetter).Select(char.ToLowerInvariant).ToArray());
                    if (key == "swingershutdown") swinger = true;
                    else if (key == "silentalarm") silent = true;
                    else if (Flag1Names.TryGetValue(key, out var fl)) flags |= fl;
                    else throw new FormatException($"unknown attribute '{a.Trim()}'");
                }
                zones.Add(new ZoneSpec(zone, f[1], def, type, area, flags, swinger, silent));
            }
            catch (Exception e) when (e is FormatException or OverflowException) { errors.Add($"line {i + 1}: {e.Message}"); }
        }
        return (zones, errors);
    }

    private static List<string> SplitCsv(string line)
    {
        var f = new List<string>(); var sb = new StringBuilder(); bool q = false;
        foreach (var c in line)
        {
            if (c == '"') q = !q;
            else if (c == ',' && !q) { f.Add(sb.ToString().Trim()); sb.Clear(); }
            else sb.Append(c);
        }
        f.Add(sb.ToString().Trim());
        return f;
    }
}
