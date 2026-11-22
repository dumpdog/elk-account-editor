using System.Globalization;

namespace Elk.Protocol;

/// <summary>Edits the zone block of an ElkRP "Export Account" file (the file ElkRP's File > Import Account reads back).
/// The file is a VB Write# sequence, one value per line. ExportAccount writes the forms in this order: account details,
/// user codes, areas, keypads, ZONES, wireless, globals, ... The zone block (Zone_Definitions.Form_Export) is:
///   count N
///   N entries, each: zone number, then 32 record bytes (one per line, same layout as <see cref="ZoneRecord"/>)
///   13 zone-group values (software 5.x and later)
/// The entries are the zones stored for the account, not necessarily 1..N (the sample account stores 1-48 and 161-176),
/// so a zone missing from the file is added as a new entry in zone-number order.
/// Only the zone lines are touched; everything else is copied through unchanged. No Windows or database needed.</summary>
public static class AccountExport
{
    public sealed record ZoneBlock(int CountLine, int Count);

    private static bool IsByte(string s) => int.TryParse(s, NumberStyles.None, CultureInfo.InvariantCulture, out var v) && v <= 255;

    /// <summary>Finds the zone block by its shape: a count, then N entries with ascending zone numbers 1..208, each followed
    /// by 32 byte values. Returns null if no unique match exists.</summary>
    public static ZoneBlock? FindZoneBlock(IReadOnlyList<string> lines)
    {
        ZoneBlock? found = null; bool tie = false;
        for (int i = 0; i + 33 < lines.Count; i++)
        {
            if (!int.TryParse(lines[i], NumberStyles.None, CultureInfo.InvariantCulture, out int n) || n < 1 || n > 208) continue;
            if (i + 1 + 33 * n > lines.Count) continue;
            bool ok = true; int prev = 0;
            for (int k = 0; k < n && ok; k++)
            {
                int p = i + 1 + 33 * k;
                if (!int.TryParse(lines[p], NumberStyles.None, CultureInfo.InvariantCulture, out int zn) || zn <= prev || zn > 208) { ok = false; break; }
                prev = zn;
                for (int b = 1; b <= 32; b++) if (!IsByte(lines[p + b])) { ok = false; break; }
            }
            if (!ok) continue;
            // Small counts (1, 2, ...) can match by accident elsewhere in the file, so the longest match wins.
            if (found == null || n > found.Count) { found = new ZoneBlock(i, n); tie = false; }
            else if (n == found.Count) tie = true;
        }
        return tie ? null : found;
    }

    private static int ZoneNumAt(IReadOnlyList<string> lines, ZoneBlock blk, int entry) => int.Parse(lines[blk.CountLine + 1 + 33 * entry], CultureInfo.InvariantCulture);

    /// <summary>Applies the specs and returns the new file text, plus a human-readable log.</summary>
    public static (string Text, List<string> Log) ApplyZones(string text, IEnumerable<ZoneSpec> specs)
    {
        string eol = text.Contains("\r\n") ? "\r\n" : "\n";
        var lines = text.Split('\n').Select(l => l.TrimEnd('\r')).ToList();
        if (lines.Count > 0 && lines[^1] == "") lines.RemoveAt(lines.Count - 1);

        var blk = FindZoneBlock(lines) ?? throw new InvalidDataException("Could not find the zone block in this export file (no unique count + ascending zone numbers + 32-byte pattern).");
        var log = new List<string> { $"zone block found at line {blk.CountLine + 1}: {blk.Count} stored zones" };
        int updated = 0, added = 0;

        foreach (var s in specs.OrderBy(s => s.Zone))
        {
            var rec = s.ToRecord();
            int entry = -1, insertAt = blk.Count;
            for (int e = 0; e < blk.Count; e++)
            {
                int zn = ZoneNumAt(lines, blk, e);
                if (zn == s.Zone) { entry = e; break; }
                if (zn > s.Zone) { insertAt = e; break; }
            }
            if (entry >= 0)
            {
                int p = blk.CountLine + 1 + 33 * entry;
                var old = new ZoneRecord();
                for (int b = 0; b < 32; b++) old.Bytes[b] = byte.Parse(lines[p + 1 + b], CultureInfo.InvariantCulture);
                rec.Bytes[3] = old.Bytes[3];            // keep the unused byte as ElkRP wrote it
                for (int b = 0; b < 32; b++) lines[p + 1 + b] = rec.Bytes[b].ToString(CultureInfo.InvariantCulture);
                log.Add($"update zone {s.Zone,3}: '{old.Name}' => '{s.Name}' def {s.Definition} type {s.Type} area {s.Area}");
                updated++;
            }
            else
            {
                var add = new List<string> { s.Zone.ToString(CultureInfo.InvariantCulture) };
                add.AddRange(rec.Bytes.Select(b => b.ToString(CultureInfo.InvariantCulture)));
                lines.InsertRange(blk.CountLine + 1 + 33 * insertAt, add);
                blk = new ZoneBlock(blk.CountLine, blk.Count + 1);
                lines[blk.CountLine] = blk.Count.ToString(CultureInfo.InvariantCulture);
                log.Add($"add    zone {s.Zone,3}: '{s.Name}' def {s.Definition} type {s.Type} area {s.Area}");
                added++;
            }
        }
        log.Add($"{updated} updated, {added} added; zone block now holds {blk.Count} zones");
        return (string.Join(eol, lines) + eol, log);
    }
}
