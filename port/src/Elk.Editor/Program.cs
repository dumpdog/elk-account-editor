using System.Diagnostics;
using System.Text;
using System.Text.Json;
using Elk.Protocol;

// Local editor for ElkRP account export files. Runs a small web server on 127.0.0.1 only and opens the page in your browser.
//   ElkAccountEditor [export-file]        then File > Import Account in ElkRP to load the saved result.
var json = new JsonSerializerOptions(JsonSerializerDefaults.Web);
string? originalText = null; string? sourceName = null;

var builder = WebApplication.CreateBuilder(new string[0]);
builder.Logging.ClearProviders();
// A fixed port keeps the page's origin the same between runs, so browser storage (drafts, remembered panel address) survives. Falls back to any free port.
int port = int.TryParse(Environment.GetEnvironmentVariable("PORT"), out var envPort) ? envPort : 51980;   // PORT is used by the rust-anything recording harness
try { var probe = new System.Net.Sockets.TcpListener(System.Net.IPAddress.Loopback, port); probe.Start(); probe.Stop(); } catch (System.Net.Sockets.SocketException) { port = 0; }
builder.WebHost.UseUrls($"http://127.0.0.1:{port}");
var app = builder.Build();

string Index() { using var s = typeof(Program).Assembly.GetManifestResourceStream("index.html")!; using var r = new StreamReader(s); return r.ReadToEnd(); }

string Template() { using var s = typeof(Program).Assembly.GetManifestResourceStream("template-m1g")!; using var r = new StreamReader(s, Encoding.Latin1); return r.ReadToEnd(); }

app.MapGet("/", () => Results.Content(Index(), "text/html; charset=utf-8"));
// A blank M1G 5.3 account (ElkRP's own default new-account export) to start from when there is no export file to open.
app.MapPost("/api/new", () =>
{
    originalText = Template(); sourceName = "New account";
    return Results.Json(new { file = sourceName, model = AccountModel.From(AccountFile.Parse(originalText)) }, json);
});
app.MapGet("/api/meta", () => Results.Json(new
{
    zoneFlags = ModelMeta.ZoneFlags, userFlags = ModelMeta.UserFlags, areaFlags = ModelMeta.AreaFlags, phoneFlags = ModelMeta.PhoneFlags,
    zoneDefinitions = ModelMeta.ZoneDefinitions, zoneTypes = ModelMeta.ZoneTypes,
    busModels = DataBusPlanner.Models, zoneExpanderSwitches = DataBusPlanner.ZoneExpanderSwitches,
    areaRcLabels = ModelMeta.AreaRcLabels, systemRcLabels = ModelMeta.SystemRcLabels, zoneRcLabels = ModelMeta.ZoneRcLabels, userRcLabels = ModelMeta.UserRcLabels,
}, json));
// Data bus planner: the plan in, the rule check and the install checklist out (Elk.Protocol/DataBusPlan.cs). Nothing here touches the panel.
app.MapPost("/api/databus/check", async (HttpRequest req) =>
{
    try { var plan = await JsonSerializer.DeserializeAsync<BusPlan>(req.Body, json) ?? new BusPlan(); return Results.Json(DataBusPlanner.Check(plan), json); }
    catch (JsonException e) { return Results.BadRequest(e.Message); }
});
app.MapGet("/api/model", () => originalText == null ? Results.NoContent() : Results.Json(new { file = sourceName, model = AccountModel.From(AccountFile.Parse(originalText)) }, json));
app.MapPost("/api/open", async (HttpRequest req) =>
{
    using var ms = new MemoryStream(); await req.Body.CopyToAsync(ms);
    var text = Encoding.Latin1.GetString(ms.ToArray());
    try { var model = AccountModel.From(AccountFile.Parse(text)); originalText = text; sourceName = req.Query["name"]; return Results.Json(new { file = sourceName, model }, json); }
    catch (Exception e) when (e is InvalidDataException or FormatException) { return Results.BadRequest(e.Message); }
});
app.MapPost("/api/save", async (HttpRequest req) =>
{
    if (originalText == null) return Results.BadRequest("Open a file first.");
    try
    {
        var model = await JsonSerializer.DeserializeAsync<AccountModel>(req.Body, json) ?? throw new FormatException("empty request");
        var f = AccountFile.Parse(originalText);
        model.ApplyTo(f);
        return Results.File(Encoding.Latin1.GetBytes(f.ToText()), "application/octet-stream");
    }
    catch (Exception e) when (e is InvalidDataException or FormatException or ArgumentOutOfRangeException or JsonException) { return Results.BadRequest(e.Message); }
});

// ---- zone lists: read names (and optionally definition, type, area) from an Excel or CSV file, and write the zones out as one --------------
// /api/zones/read returns the rows and any warnings; the page applies them to the open model, so the edit goes through undo like any other.
app.MapPost("/api/zones/read", async (HttpRequest req) =>
{
    using var ms = new MemoryStream(); await req.Body.CopyToAsync(ms);
    try
    {
        var r = ZoneSheet.Read(ms.ToArray(), (string?)req.Query["name"] ?? "");
        return Results.Json(new { rows = r.Rows.Select(x => new { n = x.N, name = x.Name, definition = x.Definition, type = x.Type, area = x.Area }), columns = r.Columns, warnings = r.Warnings }, json);
    }
    catch (Exception e) when (e is InvalidDataException or FormatException) { return Results.BadRequest(e.Message); }
});
app.MapPost("/api/zones/export", async (HttpRequest req) =>
{
    try
    {
        var model = await JsonSerializer.DeserializeAsync<AccountModel>(req.Body, json) ?? throw new FormatException("empty request");
        var table = ZoneSheet.Table(model.Zones, req.Query["all"] == "1");
        return req.Query["format"] == "csv"
            ? Results.File(ZoneSheet.ToCsv(table), "text/csv")
            : Results.File(ZoneSheet.ToXlsx(table), "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet");
    }
    catch (Exception e) when (e is FormatException or JsonException) { return Results.BadRequest(e.Message); }
});

// The Alarm Zone List ELK spreadsheet (commissioning checklist) filled from the edited model: zone name, definition code and area.
app.MapPost("/api/export-alarm-list", async (HttpRequest req) =>
{
    try
    {
        var model = await JsonSerializer.DeserializeAsync<AccountModel>(req.Body, json) ?? throw new FormatException("empty request");
        return Results.File(AlarmZoneList.Build(model, req.Query["title"].ToString() is { Length: > 0 } t ? t : null), "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet");
    }
    catch (Exception e) when (e is InvalidDataException or FormatException or JsonException) { return Results.BadRequest(e.Message); }
});

var live = new LivePanel();       // the shared panel session (see LivePanel.cs)
var progress = new PanelProgress();   // what a long panel operation (read, compare, send, backup, restore) is doing; the page polls /api/panel/progress
async Task<IResult> Tracked(Func<Task<IResult>> run) { try { return await run(); } finally { progress.End(); } }
app.MapGet("/api/panel/progress", () => Results.Json(progress.Snapshot(), json));
PanelConnection.Settings PanelSettings(PanelRequest pr) => new(pr.Host.Trim(), pr.Port, pr.Code.Trim(), pr.Plain, pr.Mac, pr.Passphrase, pr.Tls, pr.Serial?.Trim());
PanelConnection.Settings RestoreSettings(RestoreRequest pr) => new(pr.Host.Trim(), pr.Port, pr.Code.Trim(), pr.Plain, pr.Mac, pr.Passphrase, pr.Tls, pr.Serial?.Trim());
// ---- panel: compare the edited model with the panel, then optionally write what differs ------------------------------------------------
// Both calls take the edited model plus the connection. Nothing is written by /compare. /send re-reads the panel afterwards and reports what still differs.
async Task<IResult> PanelCall(HttpRequest req, bool send)
{
    if (originalText == null) return Results.BadRequest("Open a file first.");
    PanelRequest? pr;
    try { pr = await JsonSerializer.DeserializeAsync<PanelRequest>(req.Body, json); } catch (JsonException e) { return Results.BadRequest(e.Message); }
    if (pr?.Model == null || string.IsNullOrWhiteSpace(pr.Host) || string.IsNullOrWhiteSpace(pr.Code)) return Results.BadRequest("Host, RP access code and the edited data are needed.");
    AccountFile f;
    try { f = AccountFile.Parse(originalText); pr.Model.ApplyTo(f); }
    catch (Exception e) when (e is InvalidDataException or FormatException or ArgumentOutOfRangeException) { return Results.BadRequest(e.Message); }
    var log = new List<string>();
    try
    {
      progress.Begin("Connecting to the panel");
      return await live.UseAsync(PanelSettings(pr), m => { log.Add(m); progress.Note(m); }, async s =>
      {
        s.OnTransfer = progress.Tick;
        try {
        progress.Next("Comparing with the panel", PanelSync.EstimateDiffTransfers(f, pr.ResetMissing));
        var d = await PanelSync.DiffAsync(s, f, pr.ResetMissing, pr.SendLocalCode);
        int written = 0; string? backupPath = null;
        if (send)
        {
            if (d.Changes.Count > 0) { progress.Next("Saving a backup of the panel first", PanelBackup.EstimateTransfers()); backupPath = SaveBackup(await PanelBackup.ReadAsync(s), "before-send"); }   // a failed backup stops the send
            progress.Next("Writing " + d.Changes.Count + " record(s) to the panel", PanelSync.WriteTransfers(d.Changes));
            await PanelSync.ApplyAsync(s, d.Changes);
            written = d.Changes.Count;
            progress.Next("Reading back to check", PanelSync.EstimateDiffTransfers(f, pr.ResetMissing));
            d = await PanelSync.DiffAsync(s, f, pr.ResetMissing, pr.SendLocalCode);          // read back: anything still different is reported
        }
        return Results.Json(new
        {
            firmware = s.Panel?.Firmware, serial = s.Panel?.SerialHex, written, log, backupPath,
            counts = d.Counts.Select(c => new { label = c.Key, same = c.Value.Same, differ = c.Value.Differ }),
            changes = d.Changes.Select(c => new { label = c.Label, number = c.Number }), skipped = d.Skipped,
        }, json);
        } finally { s.OnTransfer = null; }
      });
    }
    catch (Exception e) when (e is TimeoutException or IOException or System.Net.Sockets.SocketException or UnauthorizedAccessException or ArgumentException or InvalidDataException)
    { return Results.BadRequest("Could not talk to the panel: " + e.Message + (log.Count > 0 ? "\n" + string.Join("\n", log) : "")); }
}
app.MapPost("/api/panel/compare", (HttpRequest req) => Tracked(() => PanelCall(req, false)));
app.MapPost("/api/panel/send", (HttpRequest req) => Tracked(() => PanelCall(req, true)));
// ---- live connection: one session kept open; the Status tab polls it and compare / send / read / backup / restore borrow it ----------------------------
IResult LiveState(LiveSnapshot? snap) => Results.Json(new { connected = live.Connected, host = live.Host, error = live.Error, firmware = live.Firmware, serial = live.Serial, snapshot = snap, at = DateTime.Now.ToString("HH:mm:ss") }, json);
app.MapPost("/api/live/connect", async (HttpRequest req) =>
{
    PanelRequest? pr;
    try { pr = await JsonSerializer.DeserializeAsync<PanelRequest>(req.Body, json); } catch (JsonException e) { return Results.BadRequest(e.Message); }
    if (pr == null || string.IsNullOrWhiteSpace(pr.Host) || string.IsNullOrWhiteSpace(pr.Code)) return Results.BadRequest("Host and RP access code are needed.");
    var log = new List<string>();
    try { await live.ConnectAsync(PanelSettings(pr), m => log.Add(m)); }
    catch (Exception e) when (e is TimeoutException or IOException or System.Net.Sockets.SocketException or UnauthorizedAccessException or ArgumentException or InvalidDataException)
    { return Results.BadRequest("Could not connect: " + e.Message + (log.Count > 0 ? "\n" + string.Join("\n", log) : "")); }
    return LiveState(await live.PollAsync());
});
// Remote control on the open connection: bypass toggle, arm / disarm, silence. These change the real panel, so the page asks for a second click first.
app.MapPost("/api/live/control", async (HttpRequest req) =>
{
    LiveControl? c;
    try { c = await JsonSerializer.DeserializeAsync<LiveControl>(req.Body, json); } catch (JsonException e) { return Results.BadRequest(e.Message); }
    if (c == null) return Results.BadRequest("Nothing to do.");
    try
    {
        var reply = await live.UseOpenAsync(s => c.Kind switch
        {
            "bypass" => PanelControl.ToggleBypassAsync(s, c.Zone, c.Area),
            "arm" => PanelControl.ArmCheckedAsync(s, c.Area, c.Level, c.Force),
            "silence" => PanelControl.SilenceAsync(s, c.Area),
            _ => throw new ArgumentException("Unknown command."),
        });
        await Task.Delay(600);
        return LiveState(await live.PollAsync());
    }
    catch (InvalidOperationException e) { return Results.BadRequest(e.Message); }
    catch (Exception e) when (e is TimeoutException or IOException or System.Net.Sockets.SocketException or ArgumentException)
    { return Results.BadRequest("The panel did not accept that: " + e.Message); }
});
// Installed bus devices and bus module enrollment (ElkRP "Enroll/Update Control and Devices"). Enroll changes which modules the panel has enrolled; the page asks for a second click first.
app.MapPost("/api/live/modules", async () =>
{
    try
    {
        var list = await live.UseOpenAsync(s => PanelModules.ScanAsync(s));
        return Results.Json(new { modules = list, note = "", at = DateTime.Now.ToString("HH:mm:ss") }, json);
    }
    catch (InvalidOperationException e) { return Results.BadRequest(e.Message); }
    catch (Exception e) when (e is TimeoutException or IOException or System.Net.Sockets.SocketException or InvalidDataException)
    { return Results.BadRequest("Could not read the devices: " + e.Message); }
});
app.MapPost("/api/live/enroll", async () =>
{
    var log = new List<string>();
    try
    {
        var (note, list) = await live.EnrollAsync(m => log.Add(m));
        return Results.Json(new { modules = list, note, at = DateTime.Now.ToString("HH:mm:ss") }, json);
    }
    catch (InvalidOperationException e) { return Results.BadRequest(e.Message); }
    catch (Exception e) when (e is TimeoutException or IOException or System.Net.Sockets.SocketException or InvalidDataException or UnauthorizedAccessException)
    { return Results.BadRequest("Enrollment did not finish: " + e.Message + (log.Count > 0 ? "\n" + string.Join("\n", log) : "")); }
});
// Panel clock, event log and RP access code (ElkRP: Set Time and Date, Receive Log, the Change button in the account details).
IResult LinkFail(Exception e, string what) => e is InvalidOperationException ? Results.BadRequest(e.Message) : Results.BadRequest(what + ": " + e.Message);
app.MapPost("/api/live/time", async () =>
{
    try { var t = await live.UseOpenAsync(s => PanelClock.ReadAsync(s)); return Results.Json(new { panel = t?.ToString("yyyy-MM-dd HH:mm:ss"), computer = DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss") }, json); }
    catch (Exception e) when (e is InvalidOperationException or TimeoutException or IOException or System.Net.Sockets.SocketException or InvalidDataException) { return LinkFail(e, "Could not read the panel clock"); }
});
app.MapPost("/api/live/settime", async (HttpRequest req) =>
{
    SetTimeRequest? r;
    try { r = await JsonSerializer.DeserializeAsync<SetTimeRequest>(req.Body, json); } catch (JsonException e) { return Results.BadRequest(e.Message); }
    DateTime when;
    if (r == null || (!r.UseComputerTime && !DateTime.TryParse(r.Time, System.Globalization.CultureInfo.InvariantCulture, System.Globalization.DateTimeStyles.None, out when))) return Results.BadRequest("Give a date and time, or use the computer's.");
    when = r.UseComputerTime ? DateTime.Now : DateTime.Parse(r.Time!, System.Globalization.CultureInfo.InvariantCulture);
    try
    {
        var ok = await live.UseOpenAsync(async s => { var acked = await PanelClock.SetAsync(s, when); return (acked, now: await PanelClock.ReadAsync(s)); });
        return Results.Json(new { acknowledged = ok.acked, sent = when.ToString("yyyy-MM-dd HH:mm:ss"), panel = ok.now?.ToString("yyyy-MM-dd HH:mm:ss"), computer = DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss") }, json);
    }
    catch (Exception e) when (e is InvalidOperationException or ArgumentOutOfRangeException or TimeoutException or IOException or System.Net.Sockets.SocketException or InvalidDataException) { return LinkFail(e, "Could not set the panel clock"); }
});
app.MapPost("/api/live/log", async (HttpRequest req) =>
{
    int max = int.TryParse((string?)req.Query["max"], out var m) ? Math.Clamp(m, 1, PanelLog.Capacity) : 20;
    try
    {
        var list = await live.UseOpenAsync(s => PanelLog.ReadAsync(s, max));
        return Results.Json(new { entries = list.Select(e => new { n = e.Index, time = e.Time?.ToString("yyyy-MM-dd HH:mm"), area = e.Area, ev = e.Event, item = e.Item }), at = DateTime.Now.ToString("HH:mm:ss") }, json);
    }
    catch (Exception e) when (e is InvalidOperationException or TimeoutException or IOException or System.Net.Sockets.SocketException or InvalidDataException) { return LinkFail(e, "Could not read the log"); }
});
app.MapPost("/api/live/accesscode", async (HttpRequest req) =>
{
    AccessCodeRequest? r;
    try { r = await JsonSerializer.DeserializeAsync<AccessCodeRequest>(req.Body, json); } catch (JsonException e) { return Results.BadRequest(e.Message); }
    if (r == null || !PanelAccess.IsValid(r.NewCode ?? "")) return Results.BadRequest("The RP access code is exactly six digits.");
    try
    {
        var ok = await live.ChangeAccessCodeAsync(r.NewCode!);
        return ok ? Results.Json(new { changed = true }, json) : Results.BadRequest("The panel did not acknowledge the change, so the code is probably unchanged. Nothing was stored.");
    }
    catch (Exception e) when (e is InvalidOperationException or TimeoutException or IOException or System.Net.Sockets.SocketException or InvalidDataException) { return LinkFail(e, "Could not change the code"); }
});
app.MapPost("/api/live/disconnect", async () => { await live.DisconnectAsync(); return LiveState(null); });
app.MapGet("/api/live/status", async () => LiveState(live.Connected ? await live.PollAsync() : null));
// ---- backups: a full copy of the panel in the CLI's JSON format, kept in Documents\ElkEditor backups -------------------------------------------------
string BackupDir() { var d = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "ElkEditor backups"); Directory.CreateDirectory(d); return d; }
string SaveBackup(PanelFile f, string why)
{
    var p = Path.Combine(BackupDir(), $"panel-{f.Serial ?? "unknown"}-{DateTime.Now:yyyyMMdd-HHmmss}-{why}.json");
    File.WriteAllText(p, f.ToJson()); return p;
}
app.MapGet("/api/panel/backups", () => Results.Json(new DirectoryInfo(BackupDir()).GetFiles("panel-*.json").OrderByDescending(f => f.LastWriteTime)
    .Select(f => new { name = f.Name, when = f.LastWriteTime.ToString("yyyy-MM-dd HH:mm:ss"), kb = f.Length / 1024 }), json));
app.MapPost("/api/panel/backup", (HttpRequest req) => Tracked(async () =>
{
    var pr = await JsonSerializer.DeserializeAsync<PanelRequest>(req.Body, json);
    if (pr == null || string.IsNullOrWhiteSpace(pr.Host) || string.IsNullOrWhiteSpace(pr.Code)) return Results.BadRequest("Host and RP access code are needed.");
    var log = new List<string>();
    try
    {
        progress.Begin("Connecting to the panel");
        var f = await live.UseAsync(PanelSettings(pr), m => { log.Add(m); progress.Note(m); }, async s =>
        {
            s.OnTransfer = progress.Tick;
            try { progress.Next("Reading the whole panel", PanelBackup.EstimateTransfers()); return await PanelBackup.ReadAsync(s); }
            finally { s.OnTransfer = null; }
        });
        var p = SaveBackup(f, "manual");
        return Results.Json(new { path = p, name = Path.GetFileName(p), zones = f.Zones.Count, blocks = f.Blocks.Count }, json);
    }
    catch (Exception e) when (e is TimeoutException or IOException or System.Net.Sockets.SocketException or UnauthorizedAccessException or ArgumentException or InvalidDataException)
    { return Results.BadRequest("Could not read the panel: " + e.Message); }
}));
// Restore: the file must be one of the saved backups (looked up by name in the backup folder). apply=false only lists what would change.
app.MapPost("/api/panel/restore", (HttpRequest req) => Tracked(async () =>
{
    var pr = await JsonSerializer.DeserializeAsync<RestoreRequest>(req.Body, json);
    if (pr == null || string.IsNullOrWhiteSpace(pr.Host) || string.IsNullOrWhiteSpace(pr.Code) || string.IsNullOrWhiteSpace(pr.File)) return Results.BadRequest("Host, RP access code and a backup file are needed.");
    var path = Path.Combine(BackupDir(), Path.GetFileName(pr.File));
    if (!File.Exists(path)) return Results.BadRequest("That backup file was not found.");
    var log = new List<string>(); var changes = new List<string>();
    try
    {
        var backup = PanelFile.FromJson(File.ReadAllText(path));
        int n = 0, left = 0;
        progress.Begin("Connecting to the panel");
        await live.UseAsync(RestoreSettings(pr), m => { log.Add(m); progress.Note(m); }, async s =>
        {
            s.OnTransfer = progress.Tick;
            try
            {
                progress.Next(pr.Apply ? "Restoring the panel from the backup" : "Comparing the panel with the backup", PanelBackup.EstimateTransfers(backup));
                n = await PanelBackup.UpdateAsync(s, backup, pr.Apply, c => changes.Add(c));
                if (pr.Apply) progress.Next("Reading back to check", PanelBackup.EstimateTransfers(backup));
                left = pr.Apply ? await PanelBackup.UpdateAsync(s, backup, false) : n;      // read back after a restore
            }
            finally { s.OnTransfer = null; }
            return 0;
        });
        return Results.Json(new { differ = n, written = pr.Apply ? n : 0, remaining = left, changes, backupFirmware = backup.Firmware, backupSerial = backup.Serial }, json);
    }
    catch (Exception e) when (e is TimeoutException or IOException or System.Net.Sockets.SocketException or UnauthorizedAccessException or ArgumentException or InvalidDataException or JsonException or FormatException)
    { return Results.BadRequest("Restore failed: " + e.Message); }
}));

// Read the panel into the open file (like ElkRP's Receive from Control): records in the file are overwritten with the panel's, clearly-used panel records are added.
app.MapPost("/api/panel/pull", (HttpRequest req) => Tracked(async () =>
{
    PanelRequest? pr;
    try { pr = await JsonSerializer.DeserializeAsync<PanelRequest>(req.Body, json); } catch (JsonException e) { return Results.BadRequest(e.Message); }
    if (pr == null || string.IsNullOrWhiteSpace(pr.Host) || string.IsNullOrWhiteSpace(pr.Code)) return Results.BadRequest("Host and RP access code are needed.");
    var log = new List<string>();
    try
    {
        if (originalText == null) { originalText = Template(); sourceName = "From panel"; }       // nothing open: read into a blank account
        var f = AccountFile.Parse(originalText);
        string? fw = null, serial = null; List<string> notes = new();
        progress.Begin("Connecting to the panel");
        await live.UseAsync(PanelSettings(pr), m => { log.Add(m); progress.Note(m); }, async s =>
        {
            s.OnTransfer = progress.Tick;
            try { progress.Next("Reading the panel", PanelSync.EstimatePullTransfers(f)); notes = await PanelSync.PullAsync(s, f, default, progress); }
            finally { s.OnTransfer = null; }
            fw = s.Panel?.Firmware; serial = s.Panel?.SerialHex; return 0;
        });
        var text = f.ToText();
        var model = AccountModel.From(AccountFile.Parse(text));
        originalText = text;
        return Results.Json(new { file = sourceName, model, notes, firmware = fw, serial }, json);
    }
    catch (Exception e) when (e is TimeoutException or IOException or System.Net.Sockets.SocketException or UnauthorizedAccessException or ArgumentException or InvalidDataException or FormatException)
    { return Results.BadRequest("Could not read the panel: " + e.Message + (log.Count > 0 ? "\n" + string.Join("\n", log) : "")); }
}));

app.Lifetime.ApplicationStopping.Register(() => { try { live.DisconnectAsync().Wait(3000); } catch { } });      // close the panel session politely
// Start synchronously so the window below runs on the main thread (macOS requires that).
app.Start();
var url = app.Urls.First();
var fileArg = args.FirstOrDefault(a => !a.StartsWith("--"));
if (fileArg != null && File.Exists(fileArg)) { originalText = File.ReadAllText(fileArg, Encoding.Latin1); sourceName = Path.GetFileName(fileArg); }
bool inBrowser = args.Contains("--browser");
if (!inBrowser)
{
    try
    {
        if (OperatingSystem.IsWindows())
        {
            // WebView2 needs a single-threaded apartment; top-level statements run in a multi-threaded one, so give the window its own STA thread.
            Exception? failure = null;
            var ui = new Thread(() => { try { NativeWindow.Run(url); } catch (Exception e) { failure = e; } });
            ui.SetApartmentState(ApartmentState.STA); ui.Start(); ui.Join();
            if (failure != null) throw failure;
        }
        else NativeWindow.Run(url);                    // macOS needs the window on the main thread
        await app.StopAsync(); return;
    }
    catch (Exception e) { Console.WriteLine("Could not open a native window (" + e.Message + "); using your browser instead."); }
}
Console.WriteLine($"Elk account editor running at {url}  (Ctrl+C to quit)");
if (!args.Contains("--no-open")) try { Process.Start(new ProcessStartInfo(url) { UseShellExecute = true }); } catch { Console.WriteLine("Open that address in your browser."); }
await app.WaitForShutdownAsync();

record SetTimeRequest(bool UseComputerTime, string? Time);
record AccessCodeRequest(string? NewCode);
record LiveControl(string Kind, int Area, int Zone = 0, int Level = 0, bool Force = false);
record RestoreRequest(string Host, int Port, string Code, bool Plain, string? Mac, string? Passphrase, string File, bool Apply, bool Tls = false, string? Serial = null);
record PanelRequest(string Host, int Port, string Code, bool Plain, string? Mac, string? Passphrase, AccountModel? Model, bool ResetMissing = false, bool SendLocalCode = false, bool Tls = false, string? Serial = null);
