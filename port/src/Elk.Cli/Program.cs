using System.Text.Json;
using System.Text.Json.Serialization;
using Elk.Protocol;

// Offline helpers:
//   elk crc <hex>                                   CRC-16 of the bytes
//   elk frame <hex>                                 show the framed (unencrypted) message
//   elk decode <zone|area|keypad> <hex>             decode a record and show its CRC
//   elk export-alarm-list --in export --out list.xlsx [--title name]   fill the Alarm Zone List spreadsheet from an export file
//   elk export-zones --in export --csv zones.csv --out new-export   edit the zones of an ElkRP account export file
// Live (AES over TCP), NOT yet tested on a real panel:
//   elk download --host H [--port 2601] --mac M --pass P --code C --out file.json
//   elk upload   --host H [--port 2601] --mac M --pass P --code C --in file.json [--yes]
//                (without --yes this only lists what would change)

static byte[] Hex(string s) => Convert.FromHexString(s.Replace(" ", ""));
static string? Opt(string[] a, string n) { int i = Array.IndexOf(a, n); return i >= 0 && i + 1 < a.Length ? a[i + 1] : null; }
static string Need(string[] a, string n) => Opt(a, n) ?? throw new ArgumentException($"missing {n}");

if (args.Length == 0) { Console.WriteLine("usage: elk crc|frame|decode|download|upload ..."); return 1; }
var json = new JsonSerializerOptions { WriteIndented = true, DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull };

switch (args[0])
{
    case "crc": Console.WriteLine($"{Crc16.Compute(Hex(args[1])):X4}"); return 0;
    case "frame": Console.WriteLine(Convert.ToHexString(Framing.Encode(Hex(args[1])))); return 0;
    case "decode":
    {
        var b = Hex(args[2]);
        switch (args[1])
        {
            case "zone": { var z = ZoneRecord.FromBytes(b); Console.WriteLine($"name='{z.Name}' function={z.Function} flag1={z.Flag1} flag2={z.Flag2} (type {ZoneFlag2.Type(z.Flag2)}, area {ZoneFlag2.Area(z.Flag2)}) crc={z.Crc}"); break; }
            case "area": { var a = new AreaRecord(); b.CopyTo(a.Bytes, 0); Console.WriteLine($"name='{a.Name}' exit1={a.Exit1} entrance1={a.Entrance1} exit2={a.Exit2} entrance2={a.Entrance2} flags1={a.Flags1} flags2={a.Flags2} armWindow={a.ArmTimeWindow} crc={a.Crc}"); break; }
            case "keypad": { var k = new KeypadRecord(); b.CopyTo(k.Bytes, 0); Console.WriteLine($"name='{k.Name}' area={k.Area} crc1={k.Crc1} crc2={k.Crc2}"); break; }
            default: Console.Error.WriteLine("zone|area|keypad"); return 1;
        }
        return 0;
    }
    case "inspect":
    {
        var f = AccountFile.Parse(File.ReadAllText(Need(args, "--in"), System.Text.Encoding.Latin1));
        Console.WriteLine($"account '{f.AccountId}' {f.PanelType} sw {f.SoftwareVersion}: {f.Users.Entries.Count} users, {f.Areas.Entries.Count} areas, {f.Keypads.Entries.Count} keypads, {f.Zones.Entries.Count} zones, {f.Phones.Entries.Count} phones");
        Console.WriteLine("zone groups: " + string.Join(",", f.Zones.Trailer));
        foreach (var (name, s) in new[] { ("user", f.Users), ("area", f.Areas), ("keypad", f.Keypads), ("phone", f.Phones) })
            foreach (var e in s.Entries.Take(args.Contains("--all") ? 999 : 4)) Console.WriteLine($"{name} {e.Number}: {Convert.ToHexString(e.Bytes)}");
        return 0;
    }
    case "export-zones":
    {
        // elk export-zones --in "Account Export" --csv zones.csv --out "New Export"
        // Fills the zones of an ElkRP "Export Account" file from a CSV. Import the result with File > Import Account.
        var (specs, errors) = ZoneCsv.Parse(File.ReadAllText(Need(args, "--csv")));
        foreach (var e in errors) Console.Error.WriteLine("CSV error: " + e);
        if (errors.Count > 0 || specs.Count == 0) { Console.Error.WriteLine("Fix the CSV first; nothing was written."); return 2; }
        var inPath = Need(args, "--in"); var outPath = Need(args, "--out");
        if (Path.GetFullPath(inPath) == Path.GetFullPath(outPath)) { Console.Error.WriteLine("--out must be a different file from --in."); return 2; }
        try
        {
            var (text, log) = AccountExport.ApplyZones(File.ReadAllText(inPath, System.Text.Encoding.Latin1), specs);
            foreach (var l in log) Console.WriteLine(l);
            File.WriteAllText(outPath, text, System.Text.Encoding.Latin1);
            Console.WriteLine($"wrote {outPath}. In ElkRP use File > Import Account.");
            return 0;
        }
        catch (InvalidDataException e) { Console.Error.WriteLine(e.Message); return 3; }
    }
    case "export-alarm-list":
    {
        // elk export-alarm-list --in "Account Export" --out "Alarm Zone List.xlsx" [--title "Site name"]
        // Fills the Alarm Zone List ELK worksheet (name, definition code and area of every zone) from an ElkRP account export file.
        var model = AccountModel.From(AccountFile.Parse(File.ReadAllText(Need(args, "--in"), System.Text.Encoding.Latin1)));
        var outPath = Need(args, "--out");
        File.WriteAllBytes(outPath, AlarmZoneList.Build(model, Opt(args, "--title")));
        Console.WriteLine($"wrote {outPath}");
        return 0;
    }
    case "download":
    {
        using var s = await OpenAsync(args);
        var f = await PanelBackup.ReadAsync(s);
        await s.DisconnectAsync();
        File.WriteAllText(Need(args, "--out"), f.ToJson());
        Console.WriteLine($"saved {f.Zones.Count} zones, {f.Areas.Count} areas, {f.Keypads.Count} keypads, {f.Blocks.Count} other blocks");
        return 0;
    }
    case "upload":
    {
        // elk upload ... --in panel.json [--yes]   restores or applies a download; without --yes it only lists what would change
        var f = PanelFile.FromJson(File.ReadAllText(Need(args, "--in")));
        bool apply = args.Contains("--yes");
        using var s = await OpenAsync(args);
        int changed = await PanelBackup.UpdateAsync(s, f, apply, Console.WriteLine);
        await s.DisconnectAsync();
        Console.WriteLine(apply ? $"wrote {changed} record(s)" : $"{changed} record(s) would change. Re-run with --yes to write them.");
        return 0;
    }
    case "upload-export":
    {
        // elk upload-export ... --in "ElkRP export file" [--yes]
        // Applies an ElkRP "Export Account" file to the panel. Only records present in the file are touched; globals and dawn/dusk are not sent.
        var f = AccountFile.Parse(File.ReadAllText(Need(args, "--in"), System.Text.Encoding.Latin1));
        bool apply = args.Contains("--yes");
        using var s = await OpenAsync(args);
        var r = await PanelSync.DiffAsync(s, f, args.Contains("--reset-missing"), args.Contains("--send-local-code"));
        foreach (var c in r.Changes) Console.WriteLine($"{c.Label} {c.Number} differs");
        if (apply) await PanelSync.ApplyAsync(s, r.Changes);
        await s.DisconnectAsync();
        foreach (var (label, c) in r.Counts) Console.WriteLine($"{label,-22} {c.Same} already match, {c.Differ} {(apply ? "written" : "would change")}");
        if (r.Skipped.Count > 0) Console.WriteLine("not on the panel (skipped): " + string.Join(", ", r.Skipped));
        if (!apply) Console.WriteLine("dry run; add --yes to write");
        return 0;
    }
    case "modules":
    {
        // elk modules ...   read only: the control, the M1XEP and every device enrolled on the data bus, with their versions
        using var s = await OpenAsync(args);
        foreach (var m in await PanelModules.ScanAsync(s)) Console.WriteLine($"{m.Kind,-22} {m.Address,2}  {m.Model,-10} hardware {m.Hardware,-6} boot {m.Boot,-8} firmware {m.Firmware}");
        await s.DisconnectAsync();
        return 0;
    }
    case "enroll":
    {
        // elk enroll ... --yes   Bus Module Enrollment (installer menu 01): the panel rescans the bus, drops the connection, then the modules are listed again
        if (!args.Contains("--yes")) { Console.WriteLine("This makes the panel rescan its data bus and re-enroll the modules it finds. Add --yes to do it."); return 0; }
        using (var s0 = await OpenAsync(args)) { Console.WriteLine(await PanelModules.EnrollAsync(s0)); try { await s0.ReadBlockAsync(7, 1); Console.WriteLine("the connection is still open after enrolling"); } catch (Exception e) { Console.WriteLine("the connection ended after enrolling (" + e.GetType().Name + ")"); } }
        Console.WriteLine("Enrolling... waiting 15 seconds"); await Task.Delay(15000);
        using var s = await OpenAsync(args);
        foreach (var m in await PanelModules.ScanAsync(s)) Console.WriteLine($"{m.Kind,-22} {m.Address,2}  {m.Model,-10} hardware {m.Hardware,-6} boot {m.Boot,-8} firmware {m.Firmware}");
        await s.DisconnectAsync();
        return 0;
    }
    case "status":
    {
        // elk status ...   read only: live status of areas, abnormal zones and system troubles
        using var s = await OpenAsync(args);
        var st = await PanelLive.ReadAsync(s);
        foreach (var a in st.Areas) Console.WriteLine($"area {a.Area}: {(a.Ready ? "ready" : "not ready")}, {a.ArmedName}{(a.Alarm ? ", ALARM" : "")}");
        foreach (var z in st.Zones.Where(z => z.State != "Normal" || z.Bypassed || z.ForceArmed).Take(40)) Console.WriteLine($"zone {z.Zone}: {z.State}{(z.Bypassed ? ", bypassed" : "")}, {z.Physical}, {z.Volts:0.0} V");
        Console.WriteLine("system: " + (st.System.Count == 0 ? "no alarms or troubles" : string.Join(", ", st.System)));
        await s.DisconnectAsync();
        return 0;
    }
    case "control":
    {
        // elk control ... --do bypass --zone N --area A | arm --area A --level 0-6 | silence --area A   CHANGES the panel state (bypass / armed state); prints the reply and the status afterwards
        var what = Need(args, "--do"); int area = int.Parse(Need(args, "--area"));
        using var s = await OpenAsync(args);
        byte[]? reply = what switch
        {
            "bypass" => await PanelControl.ToggleBypassAsync(s, int.Parse(Need(args, "--zone")), area),
            "arm" => await PanelControl.ArmCheckedAsync(s, area, int.Parse(Need(args, "--level")), args.Contains("--force")),
            "silence" => await PanelControl.SilenceAsync(s, area),
            _ => throw new ArgumentException("--do must be bypass, arm or silence"),
        };
        Console.WriteLine("reply: " + (reply == null ? "(none)" : Convert.ToHexString(reply)));
        await Task.Delay(800);
        var st = await PanelLive.ReadAsync(s);
        foreach (var a in st.Areas.Where(a => a.Area == area)) Console.WriteLine($"area {a.Area}: {(a.Ready ? "ready" : "not ready")}, {a.ArmedName}{(a.Alarm ? ", ALARM" : "")}");
        if (args.Contains("--zone")) { var zn = int.Parse(Need(args, "--zone")); var z = st.Zones[zn - 1]; Console.WriteLine($"zone {z.Zone}: {z.State}{(z.Bypassed ? ", bypassed" : "")}, {z.Physical}"); }
        await s.DisconnectAsync();
        return 0;
    }
    case "raw-read":
    {
        // elk raw-read ... --section N --item I [--part P]   read-only: prints the whole reply (hex)
        using var s = await OpenAsync(args);
        byte sec = byte.Parse(Need(args, "--section")), item = byte.Parse(Need(args, "--item")), part = byte.Parse(Opt(args, "--part") ?? "0");
        await s.SendAsync(new byte[] { sec, part, 0, item });
        var r = await s.ReceiveAsync(sec);
        Console.WriteLine($"{r.Length} bytes: {Convert.ToHexString(r)}");
        await s.DisconnectAsync();
        return 0;
    }
    case "raw-write":
    {
        // elk raw-write ... --section N --item I [--part P] --data HEX [--yes]   without --yes it only prints what it would send
        byte sec = byte.Parse(Need(args, "--section")), item = byte.Parse(Need(args, "--item")), part = byte.Parse(Opt(args, "--part") ?? "0");
        var data = Hex(Need(args, "--data"));
        Console.WriteLine($"would write section {sec} item {item} part {part}: {data.Length} bytes {Convert.ToHexString(data)}");
        if (!args.Contains("--yes")) { Console.WriteLine("dry run; add --yes to send"); return 0; }
        using var s = await OpenAsync(args);
        await s.WriteRecordAsync((Section)sec, item, data, default, part);
        await s.DisconnectAsync();
        Console.WriteLine("written");
        return 0;
    }
    default: Console.Error.WriteLine("unknown command"); return 1;
}

// section, first item, last item. The panel does not enforce item ranges (it mirrors data past the end), so these are the known limits.

static Task<ElkSession> OpenAsync(string[] a)
    => PanelConnection.OpenAsync(new PanelConnection.Settings(Need(a, "--host"), int.Parse(Opt(a, "--port") ?? (a.Contains("--plain") ? "2101" : "2601")), Need(a, "--code"), a.Contains("--plain"),
        Opt(a, "--mac"), Opt(a, "--pass"), a.Contains("--tls"), Opt(a, "--serial")), m => Console.Error.WriteLine(m));
