using System.Globalization;

namespace Elk.Protocol;

/// <summary>An ElkRP "Export Account" file (VB Write# format, one value per line) with the sections this tool edits located:
/// Users, Areas, Keypads, Zones (+ 13 wireless zone-group flags) and Telephone numbers. All other lines are kept verbatim.
/// Block layouts come from each form's Form_Export in the M1 plugin:
///   Users      count, N x (code number, 28 bytes)
///   Areas      count, N x (area number, 24 bytes)
///   Keypads    count, N x (keypad number, 148 bytes), then 16 keypad-version values
///   Zones      count, N x (zone number, 32 bytes), then 13 wireless zone-group flags (software 5.x and later)
///   Wireless   3 x 260 values (software 5.x and later) - not edited
///   Telephones count, N x (telephone id, 88 bytes)
/// Entries carry their own numbers and are stored in ascending order; a missing number is simply not stored.</summary>
public sealed class AccountFile
{
    public sealed class Entry { public int Number; public byte[] Bytes = Array.Empty<byte>(); }

    public sealed class Section
    {
        public string Name = "";
        public int RecordSize, MaxNumber, TrailerCount;
        public List<Entry> Entries = new();
        public List<string> Trailer = new();
        public Entry? Get(int n) => Entries.FirstOrDefault(e => e.Number == n);
        public Entry GetOrAdd(int n)
        {
            var e = Get(n);
            if (e != null) return e;
            if (n < 1 || n > MaxNumber) throw new ArgumentOutOfRangeException(nameof(n), $"{Name} number must be 1-{MaxNumber}");
            e = new Entry { Number = n, Bytes = new byte[RecordSize] };
            int i = Entries.FindIndex(x => x.Number > n);
            if (i < 0) Entries.Add(e); else Entries.Insert(i, e);
            return e;
        }
    }

    public string Header = "";
    public Section Users = new() { Name = "User", RecordSize = 28, MaxNumber = 255 };
    public Section Areas = new() { Name = "Area", RecordSize = 24, MaxNumber = 8 };
    public Section Keypads = new() { Name = "Keypad", RecordSize = 148, MaxNumber = 16, TrailerCount = 16 };
    public Section Zones = new() { Name = "Zone", RecordSize = 32, MaxNumber = 208, TrailerCount = 13 };
    public Section Phones = new() { Name = "Telephone", RecordSize = 88, MaxNumber = 8 };
    // Communicator ("Report Codes") block, written by Report_Codes.Form_Export: area RCs (number + 20 bytes), user RCs (number + 4),
    // zone RCs (number + 4), 24 system RC bytes, 96 keypad RC bytes (6 per keypad).
    public Section AreaRcs = new() { Name = "Area RC", RecordSize = 20, MaxNumber = 8 };
    public Section UserRcs = new() { Name = "User RC", RecordSize = 4, MaxNumber = 255 };
    public Section ZoneRcs = new() { Name = "Zone RC", RecordSize = 4, MaxNumber = 208 };
    public byte[] SystemRc = new byte[24];
    public byte[] KeypadRc = new byte[96];
    public bool HasComm => _rcAt >= 0;
    /// <summary>True when the telephone block was located (it may hold 0 entries).</summary>
    public bool HasPhoneSection => _phonesAt >= 0;
    private int _rcAt = -1, _rcLen;

    private string _eol = "\r\n";
    private List<string> _lines = new();
    private int _wirelessAt = -1, _wirelessBlocks;
    private int _usersAt, _zonesEnd, _phonesAt = -1, _globalsAt = -1, _globalsLen;   // line indexes

    /// <summary>Software version text from the account header (field 18), e.g. "5.0.12".</summary>
    public string SoftwareVersion { get; private set; } = "";
    public string PanelType { get; private set; } = "";
    public string AccountId { get; private set; } = "";
    /// <summary>User code length from Globals byte 0 bit 2 (set = 6 digits, else 4), or null if the globals could not be located.</summary>
    public int? CodeLength { get; private set; }

    private static bool IsNum(string s, int max, out int v) => int.TryParse(s, NumberStyles.None, CultureInfo.InvariantCulture, out v) && v <= max;

    /// <summary>Reads one block at line i. Returns the line after it, or -1 if the lines there do not have the block's shape.</summary>
    private static int TryBlock(List<string> L, int i, Section s, bool strictFirst, List<Entry> into)
    {
        into.Clear();
        if (i >= L.Count || !IsNum(L[i], 255, out int n) || n > s.MaxNumber) return -1;
        int p = i + 1, prev = 0;
        if (p + (s.RecordSize + 1) * n > L.Count) return -1;
        for (int k = 0; k < n; k++, p += s.RecordSize + 1)
        {
            if (!IsNum(L[p], s.MaxNumber, out int num) || num <= prev) return -1;
            prev = num;
            var b = new byte[s.RecordSize];
            for (int j = 0; j < s.RecordSize; j++) { if (!IsNum(L[p + 1 + j], 255, out int v)) return -1; b[j] = (byte)v; }
            into.Add(new Entry { Number = num, Bytes = b });
        }
        return p;
    }

    private static List<string> TakeTrailer(List<string> L, int at, int count)
        => at + count <= L.Count ? L.GetRange(at, count) : throw new InvalidDataException("file ends inside a section trailer");

    public static AccountFile Parse(string text)
    {
        var f = new AccountFile { _eol = text.Contains("\r\n") ? "\r\n" : "\n" };
        f._lines = text.Split('\n').Select(l => l.TrimEnd('\r')).ToList();
        if (f._lines.Count > 0 && f._lines[^1] == "") f._lines.RemoveAt(f._lines.Count - 1);
        var L = f._lines;
        f.Header = L.Count > 0 ? L[0] : "";
        var hf = SplitCsvLine(f.Header);
        if (hf.Count > 18) { f.AccountId = hf[0]; f.PanelType = hf[16]; f.SoftwareVersion = hf[17]; }
        bool v5 = double.TryParse(f.SoftwareVersion.Split('.')[0], out var maj) && maj >= 5;
        int zoneTrailer = v5 ? 13 : 0;

        // Users -> Areas -> Keypads -> Zones are consecutive, which makes the chain easy to find reliably. Take the match with the most entries.
        int bestAt = -1, bestScore = -1; bool tie = false;
        var tmp = new List<Entry>();
        for (int i = 1; i < L.Count; i++)
        {
            var u = new Section { RecordSize = 28, MaxNumber = 255 }; var a = new Section { RecordSize = 24, MaxNumber = 8 };
            var k = new Section { RecordSize = 148, MaxNumber = 16 }; var z = new Section { RecordSize = 32, MaxNumber = 208 };
            int e1 = TryBlock(L, i, u, false, tmp); if (e1 < 0) continue; int nu = tmp.Count;
            int e2 = TryBlock(L, e1, a, false, tmp); if (e2 < 0) continue; int na = tmp.Count;
            int e3 = TryBlock(L, e2, k, false, tmp); if (e3 < 0) continue; int nk = tmp.Count;
            int e4 = TryBlock(L, e3 + 16, z, false, tmp); if (e4 < 0) continue; int nz = tmp.Count;
            int score = nu + na + nk + nz;
            if (score > bestScore) { bestScore = score; bestAt = i; tie = false; } else if (score == bestScore) tie = true;
        }
        if (bestAt < 0 || tie) throw new InvalidDataException("Could not find the Users/Areas/Keypads/Zones sections in this file. Is it an M1 / M1G account export?");

        f._usersAt = bestAt;
        int at = bestAt;
        foreach (var s in new[] { f.Users, f.Areas, f.Keypads, f.Zones })
        {
            at = TryBlock(L, at, s, false, s.Entries);
            if (s.TrailerCount > 0)
            {
                int tc = s == f.Zones ? zoneTrailer : s.TrailerCount;
                s.Trailer = TakeTrailer(L, at, tc); at += tc;
            }
        }
        f._zonesEnd = at;

        // Telephones: after the 780 wireless values, globals and dawn/dusk (sizes not needed). Find the first block whose ids start at 1.
        int wirelessEnd = at + (v5 ? 780 : 260);
        if (wirelessEnd <= L.Count) { f._wirelessAt = at; f._wirelessBlocks = v5 ? 3 : 1; }
        if (wirelessEnd < L.Count && IsNum(L[wirelessEnd], 255, out int g0)) { f.CodeLength = (g0 & 4) != 0 ? 6 : 4; f._globalsAt = wirelessEnd; }
        // Exact position from the ElkRP exporter: Globals writes 48 values when the software string starts above "4.3" (else 36), then Dawn/Dusk writes 102
        // (lat/lon/zone/status + 12 months x 4 sunrise + 12 x 4 sunset). Try that first; the shape search below is only a fallback (it can match globals data).
        int globalsLen = string.CompareOrdinal(f.SoftwareVersion.Length >= 3 ? f.SoftwareVersion[..3] : f.SoftwareVersion, "4.3") > 0 ? 48 : 36;
        f._globalsLen = globalsLen;
        int exactPhones = wirelessEnd + globalsLen + 102;
        int searchFrom = wirelessEnd;
        if (exactPhones < L.Count)
        {
            var ph0 = new Section { RecordSize = 88, MaxNumber = 8 };
            int e0 = TryBlock(L, exactPhones, ph0, false, ph0.Entries);
            // An account with no telephones writes just "0" here; accept it so the search below cannot latch onto the report-code block that follows.
            if (e0 >= 0 && (ph0.Entries.Count == 0 || ph0.Entries[0].Number == 1)) { f._phonesAt = exactPhones; f.Phones.Entries = ph0.Entries; searchFrom = L.Count; }
        }
        for (int i = searchFrom; i < L.Count; i++)
        {
            var ph = new Section { RecordSize = 88, MaxNumber = 8 };
            int e = TryBlock(L, i, ph, false, ph.Entries);
            if (e < 0 || ph.Entries.Count < 1 || ph.Entries[0].Number != 1) continue;
            f._phonesAt = i; f.Phones.Entries = ph.Entries; break;
        }
        // Communicator block: after the telephones. Take the candidate with the most entries (the first one on a tie).
        int rcFrom = f._phonesAt >= 0 ? f._phonesAt + 1 + 89 * f.OriginalPhoneCount : wirelessEnd;
        int rcBest = -1;
        for (int i = rcFrom; i < L.Count; i++)
        {
            var a = new Section { RecordSize = 20, MaxNumber = 8 }; var u = new Section { RecordSize = 4, MaxNumber = 255 }; var z = new Section { RecordSize = 4, MaxNumber = 208 };
            int e1 = TryBlock(L, i, a, false, a.Entries); if (e1 < 0) continue;
            int e2 = TryBlock(L, e1, u, false, u.Entries); if (e2 < 0) continue;
            int e3 = TryBlock(L, e2, z, false, z.Entries); if (e3 < 0) continue;
            if (e3 + 120 > L.Count) continue;
            bool ok = true; for (int j = 0; j < 120 && ok; j++) ok = IsNum(L[e3 + j], 255, out _);
            if (!ok) continue;
            int score = a.Entries.Count + u.Entries.Count + z.Entries.Count;
            if (score <= rcBest) continue;
            rcBest = score; f._rcAt = i; f._rcLen = e3 + 120 - i;
            f.AreaRcs.Entries = a.Entries; f.UserRcs.Entries = u.Entries; f.ZoneRcs.Entries = z.Entries;
            for (int j = 0; j < 24; j++) f.SystemRc[j] = byte.Parse(L[e3 + j], CultureInfo.InvariantCulture);
            for (int j = 0; j < 96; j++) f.KeypadRc[j] = byte.Parse(L[e3 + 24 + j], CultureInfo.InvariantCulture);
        }
        return f;
    }

    /// <summary>Writes the file back, replacing the edited sections' lines and copying every other line unchanged.</summary>
    public string ToText()
    {
        var outL = new List<string> { };
        // untouched head (header + account details lines) up to the users block
        outL.AddRange(_lines.Take(_usersAt));
        void Emit(Section s, int trailerCount)
        {
            outL.Add(s.Entries.Count.ToString(CultureInfo.InvariantCulture));
            foreach (var e in s.Entries) { outL.Add(e.Number.ToString(CultureInfo.InvariantCulture)); outL.AddRange(e.Bytes.Select(b => b.ToString(CultureInfo.InvariantCulture))); }
            outL.AddRange(s.Trailer.Take(trailerCount));
        }
        Emit(Users, 0); Emit(Areas, 0); Emit(Keypads, 16); Emit(Zones, Zones.Trailer.Count);
        int pos = _zonesEnd;
        void Copy(int upTo) { if (upTo > pos) { outL.AddRange(_lines.Skip(pos).Take(upTo - pos)); pos = upTo; } }
        if (_phonesAt >= 0) { Copy(_phonesAt); Emit(Phones, 0); pos = _phonesAt + 1 + 89 * OriginalPhoneCount; }
        if (_rcAt >= 0)
        {
            Copy(_rcAt); Emit(AreaRcs, 0); Emit(UserRcs, 0); Emit(ZoneRcs, 0);
            outL.AddRange(SystemRc.Select(b => b.ToString(CultureInfo.InvariantCulture)));
            outL.AddRange(KeypadRc.Select(b => b.ToString(CultureInfo.InvariantCulture)));
            pos = _rcAt + _rcLen;
        }
        Copy(_lines.Count);
        return string.Join(_eol, outL) + _eol;
    }

    /// <summary>The wireless receiver blocks (3 x 260 values for software 5.x and later, else 1), or null if they were not located.</summary>
    public byte[][]? Wireless => _wirelessAt < 0 ? null : Enumerable.Range(0, _wirelessBlocks).Select(b => _lines.Skip(_wirelessAt + b * 260).Take(260).Select(l => byte.Parse(l, CultureInfo.InvariantCulture)).ToArray()).ToArray();

    /// <summary>Overwrites one wireless block (0-based) in place.</summary>
    public void SetWireless(int block, byte[] w)
    {
        if (_wirelessAt < 0 || block < 0 || block >= _wirelessBlocks) throw new InvalidOperationException("no such wireless block");
        if (w.Length != 260) throw new ArgumentException("a wireless block has 260 values");
        for (int i = 0; i < 260; i++) _lines[_wirelessAt + block * 260 + i] = w[i].ToString(CultureInfo.InvariantCulture);
    }

    /// <summary>The globals values (48 bytes for software above 4.3, else 36), or null if they were not located.</summary>
    public byte[]? Globals => _globalsAt < 0 ? null : _lines.Skip(_globalsAt).Take(_globalsLen).Select(l => byte.Parse(l, CultureInfo.InvariantCulture)).ToArray();

    /// <summary>Overwrites the globals lines in place (same length) and refreshes <see cref="CodeLength"/>.</summary>
    public void SetGlobals(byte[] g)
    {
        if (_globalsAt < 0) throw new InvalidOperationException("globals were not located in this file");
        if (g.Length < _globalsLen) throw new ArgumentException($"need {_globalsLen} globals bytes, got {g.Length}");
        for (int i = 0; i < _globalsLen; i++) _lines[_globalsAt + i] = g[i].ToString(CultureInfo.InvariantCulture);
        CodeLength = (g[0] & 4) != 0 ? 6 : 4;
    }

    private int OriginalPhoneCount => _phonesAt < 0 ? 0 : int.Parse(_lines[_phonesAt], CultureInfo.InvariantCulture);

    public static List<string> SplitCsvLine(string line)
    {
        var f = new List<string>(); var sb = new System.Text.StringBuilder(); bool q = false;
        foreach (var c in line)
        {
            if (c == '"') q = !q;
            else if (c == ',' && !q) { f.Add(sb.ToString()); sb.Clear(); }
            else sb.Append(c);
        }
        f.Add(sb.ToString());
        return f;
    }
}
