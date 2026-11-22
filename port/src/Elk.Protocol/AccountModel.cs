using System.Text;

namespace Elk.Protocol;

/// <summary>Friendly, JSON-serialisable view of the sections of an <see cref="AccountFile"/>. Fields that are decoded are
/// editable by name; every other byte of a record is left exactly as ElkRP wrote it.
/// Decoding comes from ElkRP's own forms (labels and bit positions from the M1 plugin); items marked "inferred" were
/// checked only against the sample export.</summary>
public sealed record FlagDef(string Key, string Label, int Byte, int Bit);

public static class ModelMeta
{
    // Communicator report-code labels, in file order (Report_Codes enums). Area RC bytes 2-18, system RC bytes 4-21.
    public static readonly string[] AreaRcLabels = { "Abort", "Cancel", "Auto close", "Early close", "Closing extended", "Fail to close", "Late close", "Key switch close", "Duress", "Exception open", "Early open", "Fail to open", "Late open", "Key switch open", "Keypad lockout", "Exit error", "Fail to communicate" };
    public static readonly string[] SystemRcLabels = { "AC fail", "AC restore", "Auto test", "Aux over-current", "Aux over-current restore", "Expansion module trouble", "Expansion module restore", "Installer on premises", "Installer off premises", "Log 80% full", "Low battery", "Low battery restore", "Memory error", "Remote programming end", "Phone line fault", "Phone line restore", "Output 2 fault", "Output 2 restore" };
    public static readonly string[] ZoneRcLabels = { "Alarm", "Restoral", "Bypass", "Trouble" };
    public static readonly string[] UserRcLabels = { "Close", "Open" };

    public static readonly FlagDef[] ZoneFlags =
    {
        new("bypassable", "Bypassable", 1, 0), new("periodicTrip", "Periodic trip", 1, 1), new("fastLoop", "Fast loop response", 1, 2),
        new("forceArm", "Force armable", 1, 3), new("chime", "Enable chime", 1, 4), new("crossZone", "In cross-zone pool", 1, 5),
        new("dialerDelay", "Use dialer delay", 1, 6), new("listenIn", "Listen in", 1, 7),
        new("swinger", "Swinger shutdown", 2, 3), new("silent", "Silent alarm", 2, 7),
    };
    public static readonly FlagDef[] UserFlags =
    {
        new("arm", "Arm", 6, 7), new("disarm", "Disarm", 6, 6), new("bypass", "Bypass", 6, 5), new("access", "Access", 6, 4),
        new("master", "Master", 6, 2), new("menus", "User menus 1-5 allowed", 6, 1), new("duress", "Duress", 7, 0), new("temporary", "Temporary code", 25, 7),
        new("area1", "Area 1", 8, 0), new("area2", "Area 2", 8, 1), new("area3", "Area 3", 8, 2), new("area4", "Area 4", 8, 3),
        new("area5", "Area 5", 8, 4), new("area6", "Area 6", 8, 5), new("area7", "Area 7", 8, 6), new("area8", "Area 8", 8, 7),
    };
    public static readonly FlagDef[] AreaFlags =
    {
        new("quickArm1", "Single keypress quick-arm", 20, 0), new("restartExit", "Restart exit timers", 20, 1), new("ringback", "Enable closing ringback", 20, 2),
        new("autoStay", "Auto Stay mode after exit time if no violation", 20, 4), new("stayKeyChange", "Allow STAY key change if armed", 20, 6),
        new("quickArm2", "Double keypress quick-arm", 21, 7), new("stay", "STAY scroll: Stay", 21, 6), new("stayInstant", "STAY scroll: Stay Instant", 21, 5),
        new("night", "STAY scroll: Night", 21, 4), new("nightInstant", "STAY scroll: Night Instant", 21, 3),
    };
    public static readonly FlagDef[] PhoneFlags =
    {
        new("areaEvents", "Area events, alarms, restorals, unbypasses", 20, 7), new("zoneBypass", "Zone bypasses", 20, 6),
        new("zoneTrouble", "Zone troubles", 20, 5), new("userEvents", "User events (opens/closings)", 20, 4), new("globalEvents", "Global system events", 20, 3),
    };

    public static readonly string[] ZoneDefinitions =
    {
        "Disabled", "Burglar Entry/Exit 1", "Burglar Entry/Exit 2", "Burglar Perimeter Instant", "Burglar Interior", "Burglar Interior Follower",
        "Burglar Interior Night", "Burglar Interior Night Delay", "Burglar 24 Hour", "Burglar Box Tamper", "Fire Alarm", "Fire Verified", "Fire Supervisory",
        "Aux1 24Hr. Alarm", "Aux2 24Hr. Alarm", "Keyfob", "Non-alarm", "Carbon Monoxide", "Emergency Alarm", "Freeze Alarm", "Gas Alarm", "Heat Alarm",
        "Medical Alarm", "Police Alarm", "Police No Indication", "Water Alarm", "Key Momentary Arm/Disarm", "Key Momentary Arm Away", "Key Momentary Arm Stay",
        "Key Momentary Disarm", "Key On/Off", "Mute Audibles", "Power Supervisory", "Temperature", "(34)", "Area Arm", "Intercom Key",
    };
    public static readonly string[] ZoneTypes =
    {
        "EOL Hardwire / Wireless", "Normally Closed", "Normally Open", "EOL Supervised on Short", "EOL Supervised on Open", "Fire / 4-Wire Smoke", "2-Wire Smoke on Zone 16",
    };
}

public sealed class ZoneDto { public int N { get; set; } public string Name { get; set; } = ""; public int Definition { get; set; } public int Type { get; set; } public int Area { get; set; } = 1; public List<string> Flags { get; set; } = new(); public bool DefaultVoice { get; set; } }
public sealed class UserDto { public int N { get; set; } public string Name { get; set; } = ""; public string Code { get; set; } = ""; public List<string> Flags { get; set; } = new(); }
public sealed class AreaDto { public int N { get; set; } public string Name { get; set; } = ""; public int Exit1 { get; set; } public int Entrance1 { get; set; } public int Exit2 { get; set; } public int Entrance2 { get; set; } public int ArmWindow { get; set; } public List<string> Flags { get; set; } = new(); }
public sealed class FKeyDto { public int Definition { get; set; } public string Name { get; set; } = ""; }
public sealed class KeypadDto { public int N { get; set; } public string Name { get; set; } = ""; public int Area { get; set; } public List<FKeyDto> FKeys { get; set; } = new(); }
public sealed class PhoneDto { public int N { get; set; } public string Name { get; set; } = ""; public string Number { get; set; } = ""; public int Format { get; set; } public int Attempts { get; set; } public string[] AccountCodes { get; set; } = new string[8]; public List<string> Flags { get; set; } = new(); }

public sealed class AreaRcDto { public int N { get; set; } public int AbortTime { get; set; } public int CancelTime { get; set; } public int[] Codes { get; set; } = new int[17]; }
public sealed class UserRcDto { public int N { get; set; } public int Close { get; set; } public int Open { get; set; } }
public sealed class ZoneRcDto { public int N { get; set; } public int[] Codes { get; set; } = new int[4]; }
public sealed class KeypadRcDto { public int N { get; set; } public int[] Codes { get; set; } = new int[6]; }
public sealed class SystemRcDto { public int AcFailDelay { get; set; } public int TestHours { get; set; } public int TestMinutes { get; set; } public int TestDays { get; set; } public int[] Codes { get; set; } = new int[18]; }
/// <summary>Communicator report codes. Every code is one byte (ElkRP shows it as two hex digits); 00 = not reported.</summary>
public sealed class CommDto
{
    public List<AreaRcDto> Areas { get; set; } = new(); public List<UserRcDto> Users { get; set; } = new(); public List<ZoneRcDto> Zones { get; set; } = new();
    public SystemRcDto System { get; set; } = new(); public List<KeypadRcDto> Keypads { get; set; } = new();
}

public sealed class AccountModel
{
    public CommDto? Comm { get; set; }
    public string Account { get; set; } = ""; public string PanelType { get; set; } = ""; public string Software { get; set; } = "";
    public int CodeLength { get; set; } = 4;
    /// <summary>The Globals values (48, or 36 for software up to 4.3) as the file has them, or null when the file has no globals. The editor changes values in place.</summary>
    public int[]? Globals { get; set; }
    /// <summary>The wireless receiver blocks, 260 values each (3 for software 5.x and later), or null. The editor changes values in place.</summary>
    public int[][]? Wireless { get; set; }
    public List<ZoneDto> Zones { get; set; } = new();
    public int[] ZoneGroups { get; set; } = Array.Empty<int>();          // 13 flags: group g = zones 16(g-1)+1 .. 16g
    public List<UserDto> Users { get; set; } = new();
    public List<AreaDto> Areas { get; set; } = new();
    public List<KeypadDto> Keypads { get; set; } = new();
    public List<PhoneDto> Phones { get; set; } = new();
    public bool HasPhones { get; set; }
    /// <summary>When true, ApplyTo also removes records that are in the file but not in this model (zones, users, areas, keypads, phones and their report-code rows).
    /// Off by default so an edit that merely lacks a record never deletes it; the editor turns it on when something is deleted.</summary>
    public bool Prune { get; set; }

    // ---- helpers -------------------------------------------------------------------------------------------------
    private static string Text(byte[] b, int at, int len) => Encoding.Latin1.GetString(b, at, len).TrimEnd('\0', ' ');
    // Only rewrites a text field when its visible text changed, so ElkRP's own padding (NUL or space) survives untouched fields.
    private static void PutText(byte[] b, int at, int len, string s)
    {
        if (Text(b, at, len) == (s.Length > len ? s[..len] : s).TrimEnd(' ')) return;
        Array.Clear(b, at, len);
        var bytes = Encoding.Latin1.GetBytes(s.Length > len ? s[..len] : s);
        Array.Copy(bytes, 0, b, at, bytes.Length);
    }
    private static List<string> GetFlags(byte[] b, FlagDef[] defs) => defs.Where(d => (b[d.Byte] & (1 << d.Bit)) != 0).Select(d => d.Key).ToList();
    private static void SetFlags(byte[] b, FlagDef[] defs, IEnumerable<string> on)
    {
        var set = on.ToHashSet();
        foreach (var d in defs) { if (set.Contains(d.Key)) b[d.Byte] |= (byte)(1 << d.Bit); else b[d.Byte] &= (byte)~(1 << d.Bit); }
    }
    private static byte Clamp(int v) => (byte)Math.Clamp(v, 0, 255);

    public static string DecodeCode(byte[] b, int len)
    {
        var sb = new StringBuilder();
        for (int i = len - 1; i >= 0; i--) sb.Append(b[i] <= 9 ? (char)('0' + b[i]) : '?');
        return sb.ToString();
    }
    public static void EncodeCode(byte[] b, string code, int len)
    {
        var digits = new string(code.Where(char.IsDigit).ToArray());
        if (digits.Length != len) throw new FormatException($"user code must be {len} digits");
        for (int i = 0; i < 6; i++) b[i] = 0;
        for (int i = 0; i < len; i++) b[i] = (byte)(digits[len - 1 - i] - '0');
    }

    // ---- decode ---------------------------------------------------------------------------------------------------
    public static AccountModel From(AccountFile f)
    {
        var m = new AccountModel { Account = f.AccountId, PanelType = f.PanelType, Software = f.SoftwareVersion, ZoneGroups = f.Zones.Trailer.Select(int.Parse).ToArray(), HasPhones = f.HasPhoneSection };
        // Code length is a Globals option (byte 0 bit 2 = 6 digits); fall back to a guess from the users if the globals were not found.
        m.Globals = f.Globals?.Select(b => (int)b).ToArray();
        m.Wireless = f.Wireless?.Select(w => w.Select(b => (int)b).ToArray()).ToArray();
        m.CodeLength = f.CodeLength ?? (f.Users.Entries.Any(e => e.Bytes[4] != 0 || e.Bytes[5] != 0) ? 6 : 4);
        foreach (var e in f.Zones.Entries)
        {
            var z = ZoneRecord.FromBytes(e.Bytes);
            m.Zones.Add(new ZoneDto { N = e.Number, Name = z.Name, Definition = z.Function, Type = ZoneFlag2.Type(z.Flag2), Area = ZoneFlag2.Area(z.Flag2), Flags = GetFlags(e.Bytes, ModelMeta.ZoneFlags) });
        }
        foreach (var e in f.Users.Entries)
            m.Users.Add(new UserDto { N = e.Number, Name = Text(e.Bytes, 9, 16), Code = DecodeCode(e.Bytes, m.CodeLength), Flags = GetFlags(e.Bytes, ModelMeta.UserFlags) });
        foreach (var e in f.Areas.Entries)
            m.Areas.Add(new AreaDto { N = e.Number, Name = Text(e.Bytes, 0, 16), Exit1 = e.Bytes[16], Entrance1 = e.Bytes[17], Exit2 = e.Bytes[18], Entrance2 = e.Bytes[19], ArmWindow = e.Bytes[22], Flags = GetFlags(e.Bytes, ModelMeta.AreaFlags) });
        foreach (var e in f.Keypads.Entries)
        {
            var k = new KeypadDto { N = e.Number, Name = Text(e.Bytes, 6, 16), Area = e.Bytes[0] };
            for (int i = 0; i < 6; i++) k.FKeys.Add(new FKeyDto { Definition = (e.Bytes[28 + 2 * i] << 8) | e.Bytes[29 + 2 * i], Name = Text(e.Bytes, 52 + 16 * i, 16) });
            m.Keypads.Add(k);
        }
        foreach (var e in f.Phones.Entries)
        {
            var p = new PhoneDto { N = e.Number, Number = Text(e.Bytes, 0, 20), Name = Text(e.Bytes, 71, 16), Format = e.Bytes[21] >> 4, Attempts = e.Bytes[22], Flags = GetFlags(e.Bytes, ModelMeta.PhoneFlags) };
            for (int a = 0; a < 8; a++) p.AccountCodes[a] = Text(e.Bytes, 23 + 6 * a, 6);
            m.Phones.Add(p);
        }
        if (f.HasComm)
        {
            var c = new CommDto();
            foreach (var e in f.AreaRcs.Entries) c.Areas.Add(new AreaRcDto { N = e.Number, AbortTime = e.Bytes[0], CancelTime = e.Bytes[1], Codes = e.Bytes.Skip(2).Take(17).Select(b => (int)b).ToArray() });
            foreach (var e in f.UserRcs.Entries) c.Users.Add(new UserRcDto { N = e.Number, Close = e.Bytes[0], Open = e.Bytes[1] });
            foreach (var e in f.ZoneRcs.Entries) c.Zones.Add(new ZoneRcDto { N = e.Number, Codes = e.Bytes.Select(b => (int)b).ToArray() });
            var sr = f.SystemRc;
            c.System = new SystemRcDto { AcFailDelay = sr[0], TestHours = sr[1], TestMinutes = sr[2], TestDays = sr[3], Codes = sr.Skip(4).Take(18).Select(b => (int)b).ToArray() };
            for (int k = 0; k < 16; k++) c.Keypads.Add(new KeypadRcDto { N = k + 1, Codes = f.KeypadRc.Skip(6 * k).Take(6).Select(b => (int)b).ToArray() });
            m.Comm = c;
        }
        return m;
    }

    private static byte Rc(int v, string what) => v is >= 0 and <= 255 ? (byte)v : throw new FormatException($"{what}: report code must be 00-FF");

    /// <summary>Elk rule (installation manual): the first wireless zone is always zone 17 and no wireless zone may exist beyond zone 160, so group 1 (zones 1-16) and groups past zone 160 cannot be wireless.
    /// Gaps are allowed: a hardwired expander enrolled in zones 33-160 uses up one wireless group of 16, and ElkRP exports such files.</summary>
    public static void ValidateWirelessGroups(int[] g)
    {
        if (g.Length == 0) return;
        if (g[0] != 0) throw new FormatException("wireless zone groups: group 1 (zones 1-16) cannot be wireless");
        for (int i = 10; i < g.Length; i++) if (g[i] != 0) throw new FormatException("wireless zone groups: no wireless zone may be above zone 160");
    }

    // ---- apply ----------------------------------------------------------------------------------------------------
    /// <summary>Writes the model into the file's sections. Records that already exist keep every byte this tool does not edit.</summary>
    public void ApplyTo(AccountFile f)
    {
        if (Wireless != null && f.Wireless is { } w0)
        {
            if (Wireless.Length != w0.Length || Wireless.Any(b => b.Length != 260 || b.Any(v => v is < 0 or > 255))) throw new FormatException("wireless: need " + w0.Length + " blocks of 260 values (0-255)");
            for (int i = 0; i < w0.Length; i++) { var nb = Wireless[i].Select(v => (byte)v).ToArray(); if (!nb.AsSpan().SequenceEqual(w0[i])) f.SetWireless(i, nb); }
        }
        if (Globals != null && f.Globals is { } g0)
        {
            if (Globals.Length != g0.Length || Globals.Any(v => v is < 0 or > 255)) throw new FormatException("globals: need " + g0.Length + " values of 0-255");
            var gb = Globals.Select(v => (byte)v).ToArray();
            if (!gb.AsSpan().SequenceEqual(g0)) f.SetGlobals(gb);
        }
        foreach (var z in Zones)
        {
            bool isNew = f.Zones.Get(z.N) == null;
            var e = f.Zones.GetOrAdd(z.N);
            if (z.Name.Length > 16 || z.Name.Any(c => c < 32 || c > 126)) throw new FormatException($"zone {z.N}: name must be at most 16 printable ASCII characters");
            if (z.Type is < 0 or > 7) throw new FormatException($"zone {z.N}: type must be 0-7");
            if (z.Area is < 1 or > 8) throw new FormatException($"zone {z.N}: area must be 1-8");
            var rec = ZoneRecord.FromBytes(e.Bytes);
            if (rec.Name != z.Name) rec.Name = z.Name;
            rec.Function = Clamp(z.Definition);
            SetFlags(rec.Bytes, ModelMeta.ZoneFlags, z.Flags);
            rec.Flag2 = ZoneFlag2.With(rec.Flag2, type: z.Type, area: z.Area);
            if (isNew || z.DefaultVoice) ZoneVoice.Apply(rec, z.N);
            rec.Bytes.CopyTo(e.Bytes, 0);
        }
        ValidateWirelessGroups(ZoneGroups);
        if (ZoneGroups.Length == f.Zones.Trailer.Count && ZoneGroups.Length > 0) f.Zones.Trailer = ZoneGroups.Select(v => v.ToString()).ToList();

        foreach (var u in Users)
        {
            var e = f.Users.GetOrAdd(u.N);
            PutText(e.Bytes, 9, 16, u.Name);
            if (!string.IsNullOrWhiteSpace(u.Code) && u.Code != DecodeCode(e.Bytes, CodeLength)) EncodeCode(e.Bytes, u.Code, CodeLength);
            SetFlags(e.Bytes, ModelMeta.UserFlags, u.Flags);
        }
        foreach (var a in Areas)
        {
            var e = f.Areas.GetOrAdd(a.N);
            PutText(e.Bytes, 0, 16, a.Name);
            e.Bytes[16] = Clamp(a.Exit1); e.Bytes[17] = Clamp(a.Entrance1); e.Bytes[18] = Clamp(a.Exit2); e.Bytes[19] = Clamp(a.Entrance2); e.Bytes[22] = Clamp(a.ArmWindow);
            SetFlags(e.Bytes, ModelMeta.AreaFlags, a.Flags);
        }
        foreach (var k in Keypads)
        {
            var e = f.Keypads.GetOrAdd(k.N);
            e.Bytes[0] = Clamp(k.Area); PutText(e.Bytes, 6, 16, k.Name);
            for (int i = 0; i < 6 && i < k.FKeys.Count; i++)
            {
                e.Bytes[28 + 2 * i] = (byte)(k.FKeys[i].Definition >> 8); e.Bytes[29 + 2 * i] = (byte)k.FKeys[i].Definition;
                PutText(e.Bytes, 52 + 16 * i, 16, k.FKeys[i].Name);
            }
        }
        foreach (var p in Phones)
        {
            var e = f.Phones.GetOrAdd(p.N);
            PutText(e.Bytes, 0, 20, p.Number); PutText(e.Bytes, 71, 16, p.Name);
            if (p.Format is < 0 or > 7) throw new FormatException($"telephone {p.N}: reporting format must be 0-7");
            e.Bytes[21] = (byte)((e.Bytes[21] & 0x0F) | (p.Format << 4)); e.Bytes[22] = Clamp(p.Attempts);
            if (p.Format == 6 && e.Bytes[22] > 1) e.Bytes[22] = 1;      // ElkRP: IP reporting allows a single attempt
            for (int a = 0; a < 8; a++) PutText(e.Bytes, 23 + 6 * a, 6, p.AccountCodes.ElementAtOrDefault(a) ?? "000000");
            SetFlags(e.Bytes, ModelMeta.PhoneFlags, p.Flags);
        }
        if (Comm != null)
        {
            if (!f.HasComm) throw new FormatException("this file has no communicator (report code) section");
            foreach (var a in Comm.Areas)
            {
                var e = f.AreaRcs.GetOrAdd(a.N); string w = $"area {a.N} report codes";
                e.Bytes[0] = Rc(a.AbortTime, w); e.Bytes[1] = Rc(a.CancelTime, w);
                for (int i = 0; i < 17 && i < a.Codes.Length; i++) e.Bytes[2 + i] = Rc(a.Codes[i], w);
            }
            foreach (var u in Comm.Users) { var e = f.UserRcs.GetOrAdd(u.N); string w = $"user {u.N} report codes"; e.Bytes[0] = Rc(u.Close, w); e.Bytes[1] = Rc(u.Open, w); }
            foreach (var z in Comm.Zones) { var e = f.ZoneRcs.GetOrAdd(z.N); for (int i = 0; i < 4 && i < z.Codes.Length; i++) e.Bytes[i] = Rc(z.Codes[i], $"zone {z.N} report codes"); }
            var s = Comm.System; const string sw = "system report codes";
            f.SystemRc[0] = Rc(s.AcFailDelay, sw); f.SystemRc[1] = Rc(s.TestHours, sw); f.SystemRc[2] = Rc(s.TestMinutes, sw); f.SystemRc[3] = Rc(s.TestDays, sw);
            for (int i = 0; i < 18 && i < s.Codes.Length; i++) f.SystemRc[4 + i] = Rc(s.Codes[i], sw);
            foreach (var k in Comm.Keypads)
                if (k.N is >= 1 and <= 16) for (int i = 0; i < 6 && i < k.Codes.Length; i++) f.KeypadRc[6 * (k.N - 1) + i] = Rc(k.Codes[i], $"keypad {k.N} report codes");
        }
        if (Prune) PruneMissing(f);
    }

    private void PruneMissing(AccountFile f)
    {
        if (Areas.Count == 0) throw new FormatException("an account needs at least one area");
        if (Keypads.Count == 0) throw new FormatException("an account needs at least one keypad");
        if (!Users.Any(u => u.N == 1)) throw new FormatException("user 1 (the master code) cannot be deleted");
        var removedAreas = f.Areas.Entries.Select(e => e.Number).Where(n => Areas.All(a => a.N != n)).ToHashSet();
        f.Zones.Entries.RemoveAll(e => Zones.All(z => z.N != e.Number));
        f.Users.Entries.RemoveAll(e => Users.All(u => u.N != e.Number));
        f.Areas.Entries.RemoveAll(e => Areas.All(a => a.N != e.Number));
        f.Keypads.Entries.RemoveAll(e => Keypads.All(k => k.N != e.Number));
        f.Phones.Entries.RemoveAll(e => Phones.All(p => p.N != e.Number));
        if (Comm != null)
        {
            f.AreaRcs.Entries.RemoveAll(e => Comm.Areas.All(a => a.N != e.Number));
            f.UserRcs.Entries.RemoveAll(e => Comm.Users.All(u => u.N != e.Number));
            f.ZoneRcs.Entries.RemoveAll(e => Comm.Zones.All(z => z.N != e.Number));
            for (int k = 1; k <= 16; k++) if (Keypads.All(x => x.N != k)) Array.Clear(f.KeypadRc, 6 * (k - 1), 6);   // a deleted keypad reports nothing
        }
        // deleting an area that zones or keypads still use would leave them pointing at nothing
        foreach (var z in Zones) if (removedAreas.Contains(z.Area)) throw new FormatException($"zone {z.N} is in area {z.Area}, which is deleted");
        foreach (var k in Keypads) if (removedAreas.Contains(k.Area)) throw new FormatException($"keypad {k.N} is in area {k.Area}, which is deleted");
    }
}
