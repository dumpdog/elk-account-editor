using System.Text.Json;

namespace Elk.Protocol;

public sealed record BlockJson(byte Section, byte Item, string Hex, byte Part = 0);
public sealed record ZoneJson(byte Number, string Name, byte Function, byte Flag1, byte Flag2, string Hex);
public sealed record AreaJson(byte Number, string Name, byte Exit1, byte Entrance1, byte Exit2, byte Entrance2, byte Flags1, byte Flags2, byte ArmTimeWindow, string Hex);
public sealed record KeypadJson(byte Number, string Name, byte Area, string Hex);

/// <summary>A full copy of the panel's programming (zones, areas, keypads and the other sections), in the JSON format the CLI's download/upload use.
/// In a restore, a record's name and settings fields are applied over its hex, so a copy can also be hand-edited.</summary>
public sealed record PanelFile
{
    public string? Firmware { get; set; }
    public string? Serial { get; set; }
    public List<ZoneJson> Zones { get; set; } = new();
    public List<AreaJson> Areas { get; set; } = new();
    public List<KeypadJson> Keypads { get; set; } = new();
    /// <summary>Other sections, compared byte for byte: users, telephones, globals, dawn/dusk and the report codes.</summary>
    public List<BlockJson> Blocks { get; set; } = new();

    public static readonly JsonSerializerOptions Json = new() { WriteIndented = true };
    public string ToJson() => JsonSerializer.Serialize(this, Json);
    public static PanelFile FromJson(string json) => JsonSerializer.Deserialize<PanelFile>(json) ?? throw new InvalidDataException("not a panel backup file");
}

public static class PanelBackup
{
    /// <summary>section, first item, last item. The panel does not enforce item ranges (it mirrors data past the end), so these are the known limits.</summary>
    public static readonly (int Section, int From, int To)[] Blocks =
    {
        (1, 1, 199),   // user codes (28 bytes)
        (8, 1, 8),     // telephone numbers (88)
        (7, 1, 2),     // globals (48) and dawn/dusk (200)
        (9, 1, 8),     // area report codes (20)
        (10, 1, 199),  // user report codes (4)
        (11, 1, 1),    // system report codes (24)
        (12, 1, 208),  // zone report codes (4)
        (16, 1, 1),    // keypad panic report codes (96)
    };

    public static string BlockLabel(int section) => section switch
    {
        1 => "user", 7 => "globals/dawn-dusk", 8 => "telephone", 9 => "area report codes", 10 => "user report codes", 11 => "system report codes",
        12 => "zone report codes", 15 => "wireless receiver", 16 => "keypad report codes", _ => "section " + section
    };

    /// <summary>Transfers a full backup read makes (208 zones, 8 areas, 16 keypads of two parts, every block and the three wireless blocks).</summary>
    public static int EstimateTransfers() => 208 + 8 + 2 * 16 + Blocks.Sum(b => b.To - b.From + 1) + 3;

    /// <summary>Transfers <see cref="UpdateAsync"/> reads when comparing with a backup (a write adds one more for every record that differs).</summary>
    public static int EstimateTransfers(PanelFile f) => f.Zones.Count + f.Areas.Count + 2 * f.Keypads.Count + f.Blocks.Count;

    public static async Task<PanelFile> ReadAsync(ElkSession s, CancellationToken ct = default)
    {
        var f = new PanelFile { Firmware = s.Panel?.Firmware, Serial = s.Panel?.SerialHex };
        for (byte n = 1; n <= 208; n++) { var z = await s.ReadZoneAsync(n, ct); f.Zones.Add(new ZoneJson(n, z.Name, z.Function, z.Flag1, z.Flag2, Convert.ToHexString(z.Bytes))); }
        for (byte n = 1; n <= 8; n++) { var a = await s.ReadAreaAsync(n, ct); f.Areas.Add(new AreaJson(n, a.Name, a.Exit1, a.Entrance1, a.Exit2, a.Entrance2, a.Flags1, a.Flags2, a.ArmTimeWindow, Convert.ToHexString(a.Bytes))); }
        for (byte n = 1; n <= 16; n++) { var k = await s.ReadKeypadAsync(n, ct); f.Keypads.Add(new KeypadJson(n, k.Name, k.Area, Convert.ToHexString(k.Bytes))); }
        foreach (var (sec, from, to) in Blocks)
            for (int n = from; n <= to; n++) f.Blocks.Add(new BlockJson((byte)sec, (byte)n, Convert.ToHexString(await s.ReadBlockAsync((byte)sec, (byte)n, 0, ct))));
        for (byte part = 0; part < 3; part++)          // wireless receiver blocks: section 15, the part selects the block, the item number is ignored
            f.Blocks.Add(new BlockJson(15, 1, Convert.ToHexString(await s.ReadBlockAsync(15, 1, part, ct)), part));
        return f;
    }

    /// <summary>Compares <paramref name="f"/> with the panel and, when <paramref name="apply"/> is set, writes what differs. Returns the number of records that differ.
    /// Zones and areas are compared by the panel's stored CRC, keypads and the other sections byte for byte.</summary>
    public static async Task<int> UpdateAsync(ElkSession s, PanelFile f, bool apply, Action<string>? log = null, CancellationToken ct = default)
    {
        int changed = 0;
        foreach (var j in f.Zones)
        {
            var z = ZoneRecord.FromBytes(Convert.FromHexString(j.Hex)); z.Name = j.Name; z.Function = j.Function; z.Flag1 = j.Flag1; z.Flag2 = j.Flag2;
            if (await s.QueryCrcAsync(Section.Zone, j.Number, ct) == z.Crc) continue;
            changed++; log?.Invoke($"zone {j.Number}: '{j.Name}' differs from panel");
            if (apply) await s.WriteRecordAsync(Section.Zone, j.Number, z.Bytes, ct);
        }
        foreach (var j in f.Areas)
        {
            var a = new AreaRecord(); Convert.FromHexString(j.Hex).CopyTo(a.Bytes, 0);
            a.Name = j.Name; a.Exit1 = j.Exit1; a.Entrance1 = j.Entrance1; a.Exit2 = j.Exit2; a.Entrance2 = j.Entrance2; a.Flags1 = j.Flags1; a.Flags2 = j.Flags2; a.ArmTimeWindow = j.ArmTimeWindow;
            if (await s.QueryCrcAsync(Section.Area, j.Number, ct) == a.Crc) continue;
            changed++; log?.Invoke($"area {j.Number}: '{j.Name}' differs from panel");
            if (apply) await s.WriteRecordAsync(Section.Area, j.Number, a.Bytes, ct);
        }
        foreach (var j in f.Keypads)
        {
            var k = new KeypadRecord(); Convert.FromHexString(j.Hex).CopyTo(k.Bytes, 0); k.Name = j.Name; k.Area = j.Area;
            var panelK = await s.ReadKeypadAsync(j.Number, ct);
            if (panelK.Bytes.AsSpan().SequenceEqual(k.Bytes)) continue;
            changed++; log?.Invoke($"keypad {j.Number}: '{j.Name}' differs from panel");
            if (apply) await s.WriteKeypadAsync(j.Number, k, ct);
        }
        foreach (var b in f.Blocks)
        {
            var want = Convert.FromHexString(b.Hex); var have = await s.ReadBlockAsync(b.Section, b.Item, b.Part, ct);
            if (have.AsSpan().SequenceEqual(want)) continue;
            changed++; log?.Invoke($"{BlockLabel(b.Section)} {b.Item}{(b.Part > 0 || b.Section == 15 ? " part " + b.Part : "")} differs from panel");
            if (apply) await s.WriteBlockAsync(b.Section, b.Item, want, b.Part, ct);
        }
        return changed;
    }
}
