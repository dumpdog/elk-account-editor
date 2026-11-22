using System.Globalization;

namespace Elk.Protocol;

// A data bus plan for an Elk M1 system and the rules it is checked against.
//
// Sources: ELK's page "5 Tips for the M1 Data Bus" (device types, no duplicate addresses within a type, no more than 2 runs on the control, exactly 2 termination points, daisy chain
// only, data bus hubs), the ELK-M1DBH and ELK-M1DBHR instruction sheets (hub wiring, termination, the 4,000 ft limit, 2 hubs of the retrofit type), and the zone expander address tables
// (docs/ELK-ZONE-EXPANDER-TABLES.md). The video that goes with the page could not be watched, so tips given only there are not in here yet.

/// <summary>A device on the bus. <see cref="Ma"/> is the current it draws from the bus supply; 0 means "not entered".</summary>
public sealed class BusDevice
{
    public string Model { get; set; } = "Keypad"; public int Address { get; set; } = 1; public string Name { get; set; } = ""; public double Ma { get; set; }
}

/// <summary>A cable run with devices in daisy chain order, nearest the hub or control first. The last device is the end of the run.</summary>
public sealed class BusRun
{
    public string Name { get; set; } = ""; public double LengthFt { get; set; } public List<BusDevice> Devices { get; set; } = new();
}

/// <summary>An ELK-M1DBH. Home runs go in jacks J1, J2 ... in order; <see cref="CrossoverFt"/> is the length of the crossover cable to the next hub (when there is one).</summary>
public sealed class BusHub
{
    public List<BusRun> HomeRuns { get; set; } = new(); public double CrossoverFt { get; set; }
    /// <summary>Last hub only. The bus normally ends with the 120 ohm plug in the first free jack. Set this to end it instead with the terminating jumper on the device on the last jack (always the case when all nine jacks are used).</summary>
    public bool TerminateAtDevice { get; set; }
    public bool EndsAtDevice(bool last) => last && HomeRuns.Count > 0 && (TerminateAtDevice || HomeRuns.Count >= 9);
}

/// <summary>One of the four managed branches of an ELK-M1DBHR: one or two home runs.</summary>
public sealed class BusBranch { public List<BusRun> Runs { get; set; } = new(); }

/// <summary>One of the (at most two) runs that connect to the control's data bus. Kind: "direct" (a daisy chain), "dbh" (a chain of M1DBH hubs) or "dbhr" (one M1DBHR).</summary>
public sealed class MainRun
{
    public string Kind { get; set; } = "direct"; public string Name { get; set; } = "";
    public BusRun Run { get; set; } = new();                          // direct
    public double FeedFt { get; set; } = 1;                           // cable from the control to the first hub
    public List<BusHub> Hubs { get; set; } = new();                   // dbh
    public List<BusBranch> Branches { get; set; } = new();            // dbhr: four
}

public sealed class BusPlan
{
    public List<MainRun> Runs { get; set; } = new();
    /// <summary>Zone groups (2 to 13) that the account has as wireless, so hardwired expanders can be checked against them. Optional.</summary>
    public List<int> WirelessGroups { get; set; } = new();
}

public sealed record BusModel(string Id, string Label, int Type, int MinAddress, int MaxAddress, string Note);
public sealed record PlanIssue(string Level, string Text, string Where = "");
public sealed record ChecklistGroup(string Title, List<string> Items);
public sealed class PlanResult
{
    public List<PlanIssue> Issues { get; set; } = new(); public List<ChecklistGroup> Checklist { get; set; } = new();
    public double MainBusFt { get; set; } public double LimitFt { get; set; } = DataBusPlanner.MaxBusFt; public double CurrentMa { get; set; } public bool Jp3 { get; set; }
    public int Devices { get; set; } public int Errors => Issues.Count(i => i.Level == "error"); public int Warnings => Issues.Count(i => i.Level == "warn");
}

public static class DataBusPlanner
{
    public const double MaxBusFt = 4000, PtcMa = 1250, HubFeedRibbonIn = 24;

    /// <summary>The device types ("Type 1" ...) are ELK's grouping: an address may be used once per type. Address limits: keypads 1-16 and zone expanders 2-13 (address tables), output expanders 1-13 and serial expanders 1-7 (ElkRP's valid device table).</summary>
    public static readonly BusModel[] Models =
    {
        new("Keypad", "Keypad (M1KP2, M1KP3, M1KPNAV, ...)", 1, 1, 16, "Type 1"),
        new("M1KAM", "M1KAM access module", 1, 1, 16, "Type 1: shares the address list with the keypads"),
        new("PowerSupply", "Supervised power supply", 1, 1, 16, "Type 1: shares the address list with the keypads"),
        new("M1XIN", "M1XIN hardwired input expander (16 zones)", 2, 2, 13, "Type 2: the address sets the 16 zones it uses (address 2 = zones 17-32)"),
        new("M1XRFTWM", "M1XRFTWM wireless transceiver", 2, 2, 5, "Type 2: primary at address 2, up to three more at 3, 4 and 5"),
        new("M1XRFEG", "M1XRFEG wireless input expander", 2, 2, 13, "Type 2 (address range not confirmed)"),
        new("M1XRF2H", "M1XRF2H wireless input expander", 2, 2, 13, "Type 2 (address range not confirmed)"),
        new("M1XOVR", "M1XOVR output expander", 3, 1, 13, "Type 3"),
        new("M1XSP", "M1XSP serial port expander", 5, 1, 7, "Type 5"),
        new("M1XSLU", "M1XSLU serial lighting expander", 5, 1, 7, "Type 5"),
        new("M1PCSPIM", "M1PCSPIM power line interface", 5, 1, 7, "Type 5"),
    };

    public static BusModel? Model(string id) => Models.FirstOrDefault(m => string.Equals(m.Id, id, StringComparison.OrdinalIgnoreCase));

    /// <summary>S1 to S4 for a hardwired expander (M1XIN) address, from the zone expander table.</summary>
    public static readonly Dictionary<int, string> ZoneExpanderSwitches = new()
    {
        [2] = "S1 off, S2 on, S3 off, S4 off", [3] = "S1 on, S2 on, S3 off, S4 off", [4] = "S1 off, S2 off, S3 on, S4 off", [5] = "S1 on, S2 off, S3 on, S4 off",
        [6] = "S1 off, S2 on, S3 on, S4 off", [7] = "S1 on, S2 on, S3 on, S4 off", [8] = "S1 off, S2 off, S3 off, S4 on", [9] = "S1 on, S2 off, S3 off, S4 on",
        [10] = "S1 off, S2 on, S3 off, S4 on", [11] = "S1 on, S2 on, S3 off, S4 on", [12] = "S1 off, S2 off, S3 on, S4 on", [13] = "S1 on, S2 off, S3 on, S4 on",
    };
    public static readonly Dictionary<int, string> TransceiverSwitches = new()
    {
        [2] = "S1 off, S2 on, S3 off, S4 off", [3] = "S1 on, S2 off, S3 off, S4 off", [4] = "S1 off, S2 off, S3 on, S4 off", [5] = "S1 on, S2 off, S3 on, S4 off",
    };

    /// <summary>Zones a hardwired expander at this address uses.</summary>
    public static (int From, int To) ZonesAt(int address) => (16 * (address - 1) + 1, 16 * address);

    private static string F(double v) => v.ToString("0.#", CultureInfo.InvariantCulture);
    private static string Dev(BusDevice d) => $"{d.Model}{(string.IsNullOrWhiteSpace(d.Name) ? "" : " \"" + d.Name.Trim() + "\"")} (address {d.Address})";

    public static PlanResult Check(BusPlan plan)
    {
        var r = new PlanResult(); var issues = r.Issues;
        void Err(string t, string w = "") => issues.Add(new PlanIssue("error", t, w));
        void Warn(string t, string w = "") => issues.Add(new PlanIssue("warn", t, w));
        void Info(string t, string w = "") => issues.Add(new PlanIssue("info", t, w));

        var runs = plan.Runs ?? new();
        var all = new List<(BusDevice D, string Where)>();
        if (runs.Count == 0) { Info("Add a run to start: a plain daisy chain, a data bus hub (M1DBH) or a retrofit hub (M1DBHR)."); return r; }
        if (runs.Count > 2) Err($"{runs.Count} runs connect to the control's data bus. No more than 2 runs may connect directly (the screw terminals and the J1 and J2 connectors all count).", "plan");

        // ---- per run structure, lengths, terminations --------------------------------------------------------------------------------------
        double mainFt = 0; int hubs = 0, dbhrCount = 0;
        for (int i = 0; i < runs.Count; i++)
        {
            var m = runs[i]; string w = "run" + (i + 1), label = $"Run {i + 1}" + (string.IsNullOrWhiteSpace(m.Name) ? "" : $" \"{m.Name.Trim()}\"");
            switch ((m.Kind ?? "").ToLowerInvariant())
            {
                case "direct":
                    if (m.Run.Devices.Count == 0) Err($"{label} has no devices. A run needs at least one, and its last device is the one that is terminated.", w);
                    if (m.Run.LengthFt <= 0) Warn($"{label}: enter the cable length so the {F(MaxBusFt)} ft limit can be checked.", w);
                    mainFt += Math.Max(0, m.Run.LengthFt);
                    foreach (var d in m.Run.Devices) all.Add((d, w));
                    if (m.Run.Devices.Count > 1) Info($"{label}: wire the {m.Run.Devices.Count} devices as a daisy chain, one after another. Star and T connections are not allowed.", w);
                    break;
                case "dbh":
                    if (m.Hubs.Count == 0) { Err($"{label} has no M1DBH hub.", w); break; }
                    if (m.FeedFt <= 0) Warn($"{label}: enter the length of the cable from the control to the first hub.", w);
                    mainFt += Math.Max(0, m.FeedFt);
                    if (m.FeedFt > HubFeedRibbonIn / 12.0 * 2) Warn($"{label}: the manual recommends the M1DBH close to the control (the ELK-W018B ribbon cable is for under {HubFeedRibbonIn} inches). The feed cable here is {F(m.FeedFt)} ft: use a dedicated 4-wire cable, 22 AWG or heavier.", w);
                    for (int h = 0; h < m.Hubs.Count; h++)
                    {
                        var hub = m.Hubs[h]; hubs++; bool last = h == m.Hubs.Count - 1; string hw = $"{w}/hub{h + 1}", hl = $"{label}, hub {h + 1}";
                        if (hub.HomeRuns.Count == 0) Warn($"{hl} has no home runs.", hw);
                        // nine RJ45 jacks: J1-J9. A hub that is not the last needs the jack after its last cable for the crossover cable. The last hub ends the bus with the 120 ohm plug in the
                        // jack after its last cable, or with the terminating jumper on the device on its last jack (the only way when all nine jacks are used).
                        if (hub.HomeRuns.Count > 9) Err($"{hl} has {hub.HomeRuns.Count} home runs. The hub has nine jacks (add a hub for more).", hw);
                        else if (!last && hub.HomeRuns.Count > 8) Err($"{hl} has {hub.HomeRuns.Count} home runs. The jack after the last cable must be free for the crossover cable to the next hub, so use at most 8 (move one home run to the next hub).", hw);
                        if (hub.EndsAtDevice(last))
                        {
                            var lastRun = hub.HomeRuns[^1];
                            if (lastRun.Devices.Count == 0) Err($"{hl}: the bus ends at the device on the last jack, but home run \"{lastRun.Name}\" has no device to carry the terminating jumper.", hw);
                            else Info($"{hl}: the bus ends at the device on the last jack, {Dev(lastRun.Devices[^1])}. Fit its terminating jumper instead of the plug, and terminate no other device.", hw);
                        }
                        foreach (var hr in hub.HomeRuns)
                        {
                            if (hr.Devices.Count == 0) Err($"{hl}: home run \"{hr.Name}\" has no device.", hw);
                            if (hr.LengthFt <= 0) Warn($"{hl}: enter the length of home run \"{hr.Name}\".", hw);
                            if (hr.Devices.Count > 1) Info($"{hl}: home run \"{hr.Name}\" has {hr.Devices.Count} devices. A home run goes to one device location; devices at the same location share its terminals. Devices at different places need their own home runs.", hw);
                            mainFt += 2 * Math.Max(0, hr.LengthFt);          // the data lines go out and come back, so every home run counts double
                            foreach (var d in hr.Devices) all.Add((d, hw));
                        }
                        if (!last) { if (hub.CrossoverFt <= 0) Warn($"{hl}: enter the length of the crossover cable to hub {h + 2}.", hw); mainFt += 2 * Math.Max(0, hub.CrossoverFt); }
                    }
                    if (m.Hubs.Count > 1) Info($"{label}: hubs are linked in one straight daisy chain with a crossover cable. Never connect hubs as spokes off one hub.", w);
                    break;
                case "dbhr":
                    dbhrCount++; hubs++;
                    if (m.FeedFt <= 0) Warn($"{label}: enter the length of the cable from the control to the retrofit hub.", w);
                    if (m.FeedFt > 10) Err($"{label}: the M1DBHR must be mounted inside the M1 enclosure and cannot be remoted. {F(m.FeedFt)} ft of feed cable is too far.", w);
                    else if (m.FeedFt > 3) Warn($"{label}: the M1DBHR is connected to the control with a short 4-conductor cable and mounted in the M1 enclosure ({F(m.FeedFt)} ft is long).", w);
                    mainFt += Math.Max(0, m.FeedFt);
                    if (m.Branches.Count != 4) Err($"{label}: the M1DBHR has exactly 4 branches (found {m.Branches.Count}).", w);
                    for (int b = 0; b < m.Branches.Count; b++)
                    {
                        var br = m.Branches[b]; string bw = $"{w}/branch{b + 1}", bl = $"{label}, branch {b + 1}";
                        if (br.Runs.Count > 2) Err($"{bl} has {br.Runs.Count} home runs. A branch can have at most 2.", bw);
                        double bft = 0;
                        foreach (var hr in br.Runs)
                        {
                            if (hr.Devices.Count == 0) Err($"{bl}: home run \"{hr.Name}\" has no device to terminate.", bw);
                            if (hr.LengthFt <= 0) Warn($"{bl}: enter the length of home run \"{hr.Name}\".", bw);
                            bft += Math.Max(0, hr.LengthFt);
                            foreach (var d in hr.Devices) all.Add((d, bw));
                        }
                        if (bft > MaxBusFt) Err($"{bl} is {F(bft)} ft of cable. The longest a branch may be is {F(MaxBusFt)} ft.", bw);
                        else if (bft > 0.8 * MaxBusFt) Warn($"{bl} is {F(bft)} ft, close to the {F(MaxBusFt)} ft limit. Use 18 to 22 AWG (24 AWG is the minimum).", bw);
                    }
                    break;
                default: Err($"{label}: unknown run type \"{m.Kind}\".", w); break;
            }
        }
        if (dbhrCount > 2) Err("More than two M1DBHR hubs on one control. The most allowed is two, and an M1DBHR cannot sit on a branch of another or act as an extender.", "plan");
        r.MainBusFt = mainFt; r.Devices = all.Count;
        if (mainFt > MaxBusFt) Err($"The data bus is {F(mainFt)} ft (hub home runs count double). The most allowed is {F(MaxBusFt)} ft.", "plan");
        else if (mainFt > 0.8 * MaxBusFt) Warn($"The data bus is {F(mainFt)} ft, close to the {F(MaxBusFt)} ft limit.", "plan");
        r.Jp3 = runs.Count == 1;

        // ---- addresses: one use per device type ------------------------------------------------------------------------------------------
        foreach (var (d, w) in all)
        {
            var m = Model(d.Model);
            if (m == null) { Err($"Unknown device model \"{d.Model}\".", w); continue; }
            if (d.Address < m.MinAddress || d.Address > m.MaxAddress)
                Err($"{Dev(d)}: {m.Label.Split('(')[0].Trim()} addresses run from {m.MinAddress} to {m.MaxAddress}.", w);
        }
        foreach (var g in all.Where(x => Model(x.D.Model) != null).GroupBy(x => (Model(x.D.Model)!.Type, x.D.Address)).Where(g => g.Count() > 1))
            Err($"Address {g.Key.Address} is used by {g.Count()} Type {g.Key.Type} devices: {string.Join(", ", g.Select(x => Dev(x.D)))}. Within a type every address must be different (a keypad and an M1XIN may share one, a keypad and an M1KAM may not).", g.First().Where);

        // ---- wireless transceivers and hardwired expanders ----------------------------------------------------------------------------------
        var tw = all.Where(x => string.Equals(x.D.Model, "M1XRFTWM", StringComparison.OrdinalIgnoreCase)).ToList();
        if (tw.Count > 4) Err($"{tw.Count} M1XRFTWM transceivers. Up to 4 may be installed.", "plan");
        if (tw.Count > 0 && tw.All(x => x.D.Address != 2)) Err("The primary M1XRFTWM transceiver must be at address 2; the additional ones use 3, 4 and 5.", tw[0].Where);
        var xin = all.Where(x => string.Equals(x.D.Model, "M1XIN", StringComparison.OrdinalIgnoreCase)).ToList();
        foreach (var (d, w) in xin.Where(x => x.D.Address is >= 2 and <= 13))
        {
            var (z0, z1) = ZonesAt(d.Address);
            if (plan.WirelessGroups.Contains(d.Address)) Err($"{Dev(d)} uses zones {z0}-{z1}, which this account has set as wireless. Move the expander or change the wireless zone groups.", w);
        }
        if (tw.Count > 0)
        {
            int lost = xin.Count(x => x.D.Address is >= 3 and <= 10);
            Info($"With wireless in use, each hardwired expander at address 3 to 10 (zones 33-160) uses up one group of 16 wireless zones. {lost} of those here: at most {Math.Max(0, 9 - lost) * 16} wireless zones remain (up to 144 without any).");
        }

        // ---- power -----------------------------------------------------------------------------------------------------------------------------
        double ma = all.Sum(x => Math.Max(0, x.D.Ma)); int active = 0;
        foreach (var m in runs.Where(m => string.Equals(m.Kind, "dbhr", StringComparison.OrdinalIgnoreCase)))
        { int br = m.Branches.Count(b => b.Runs.Count > 0); active += br; ma += 30 + 20 * br; }
        r.CurrentMa = ma;
        if (all.Any(x => x.D.Ma > 0) || active > 0)
        {
            if (ma > PtcMa) Err($"The devices draw about {F(ma)} mA. The control's +12V keypad output is protected by a 1.25 A fuse (PTC); use an auxiliary 12 V supply for part of the bus.", "plan");
            else if (ma > 0.8 * PtcMa) Warn($"The devices draw about {F(ma)} mA, near the 1.25 A the control's +12V keypad output can supply.", "plan");
        }
        else if (all.Count > 0) Info("No device currents entered, so the power load was not checked. The +12V keypad output of the control is protected at 1.25 A; the hub adds 30 mA, plus 20 mA per branch for the M1DBHR.");

        // ---- advice ------------------------------------------------------------------------------------------------------------------------------
        if (runs.Any(x => string.Equals(x.Kind, "direct", StringComparison.OrdinalIgnoreCase)) && all.Count >= 4)
            Info("A data bus hub is the preferred way to wire a bus with several devices (new wiring: M1DBH with CAT5/6 home runs; reusing 4-conductor cable: M1DBHR). A direct bus is limited to two runs.");
        BuildChecklist(plan, r, all);
        return r;
    }

    private static void BuildChecklist(BusPlan plan, PlanResult r, List<(BusDevice D, string Where)> all)
    {
        var runs = plan.Runs; var L = r.Checklist;
        var ctrl = new List<string>
        {
            runs.Count == 1 ? "JP3 (terminating jumper on the M1 control): INSTALL it. With one run, the control end is the second of the two termination points."
                            : "JP3 (terminating jumper on the M1 control): REMOVE it. With two runs, the far end of each run is terminated instead.",
            "The data bus must have exactly two terminating points in total, never more.",
        };
        L.Add(new ChecklistGroup("Control", ctrl));
        for (int i = 0; i < runs.Count; i++)
        {
            var m = runs[i]; var items = new List<string>(); string title = $"Run {i + 1}" + (string.IsNullOrWhiteSpace(m.Name) ? "" : $": {m.Name.Trim()}");
            switch ((m.Kind ?? "").ToLowerInvariant())
            {
                case "direct":
                    items.Add($"Daisy chain: control → {(m.Run.Devices.Count == 0 ? "(no devices)" : string.Join(" → ", m.Run.Devices.Select(Dev)))}. About {F(m.Run.LengthFt)} ft.");
                    if (m.Run.Devices.Count > 0) items.Add($"Terminate the last device only: {Dev(m.Run.Devices[^1])}. Fit its terminating jumper. Do not terminate the others.");
                    break;
                case "dbh":
                    items.Add($"Connect hub 1 to the control's +12V, DATA A, DATA B and NEG terminals with a dedicated 4-wire cable, 22 AWG minimum ({F(m.FeedFt)} ft). The optional ELK-W018B ribbon cable can be used if the hub is under 24 inches from the board.");
                    for (int h = 0; h < m.Hubs.Count; h++)
                    {
                        var hub = m.Hubs[h]; bool last = h == m.Hubs.Count - 1;
                        for (int j = 0; j < hub.HomeRuns.Count; j++)
                            items.Add($"Hub {h + 1}, J{j + 1}: home run \"{hub.HomeRuns[j].Name}\" ({F(hub.HomeRuns[j].LengthFt)} ft) to {(hub.HomeRuns[j].Devices.Count == 0 ? "(no device)" : string.Join(" + ", hub.HomeRuns[j].Devices.Select(Dev)))}.");
                        int free = hub.HomeRuns.Count + 1;
                        if (hub.EndsAtDevice(last))
                        {
                            var ld = hub.HomeRuns[^1].Devices.Count > 0 ? Dev(hub.HomeRuns[^1].Devices[^1]) : "the device on the last jack";
                            items.Add($"Hub {h + 1}: no terminating plug. Fit the terminating jumper on the device on the last jack (J{hub.HomeRuns.Count}): {ld}. It is the only termination on the hub side; terminate no other device.");
                        }
                        else items.Add(last ? $"Hub {h + 1}, J{free}: insert the RJ45 terminating plug (120 Ω, supplied). It is the only termination on the hub side. Do not terminate any device."
                                       : $"Hub {h + 1}, J{free}: crossover cable (orange and green pairs swapped) to J1 of hub {h + 2}. No terminating plug on this hub.");
                    }
                    items.Add("Fill jacks from J1 upward without skipping one. Use continuous CAT5/6 from hub to device, no splices and no other cable type. Wire to the device terminals with the T568A colours (below).");
                    break;
                case "dbhr":
                    items.Add($"Mount the M1DBHR inside the M1 enclosure. Connect its +, A, B and − terminals to the control's +VKP, DATA A, DATA B and NEG with a short 4-conductor cable ({F(m.FeedFt)} ft). If the hub is powered from an auxiliary supply, do not connect +VKP and join all the negatives.");
                    items.Add("JP1 (terminates the input from the control): INSTALL it.");
                    for (int b = 0; b < m.Branches.Count; b++)
                    {
                        var br = m.Branches[b]; string jp = "JP" + (b + 2);
                        if (br.Runs.Count == 0) { items.Add($"Branch {b + 1}: not used. Leave {jp} as it is; no devices are connected."); continue; }
                        items.Add($"Branch {b + 1}: " + string.Join("; ", br.Runs.Select(hr => $"home run \"{hr.Name}\" ({F(hr.LengthFt)} ft): " + (hr.Devices.Count == 0 ? "(no devices)" : string.Join(" → ", hr.Devices.Select(Dev))))) + ".");
                        items.Add(br.Runs.Count == 1
                            ? $"Branch {b + 1}: terminate the last device ({(br.Runs[0].Devices.Count == 0 ? "?" : Dev(br.Runs[0].Devices[^1]))}) AND fit {jp} on the hub. That is the branch's two terminations."
                            : $"Branch {b + 1}: terminate the last device of each home run and REMOVE {jp}.");
                    }
                    break;
            }
            L.Add(new ChecklistGroup(title, items));
        }
        var sw = new List<string>();
        foreach (var (d, _) in all.OrderBy(x => Model(x.D.Model)?.Type).ThenBy(x => x.D.Address))
        {
            var m = Model(d.Model); if (m == null) continue;
            string extra = "";
            if (m.Id == "M1XIN" && ZoneExpanderSwitches.TryGetValue(d.Address, out var s1)) { var (a, b) = ZonesAt(d.Address); extra = $": zones {a}-{b}; {s1}"; }
            else if (m.Id == "M1XRFTWM" && TransceiverSwitches.TryGetValue(d.Address, out var s2)) extra = $": {(d.Address == 2 ? "primary" : "additional")} transceiver; {s2}";
            sw.Add($"{Dev(d)}{extra}");
        }
        if (sw.Count > 0) { sw.Add("After setting the addresses, run Bus Module Enrollment (installer programming, or Live > Modules in this editor). Devices added later must be enrolled too."); L.Add(new ChecklistGroup("Addresses to set", sw)); }
        if (runs.Any(x => x.Kind is "dbh"))
            L.Add(new ChecklistGroup("CAT5/6 colours (T568A, hub to device)", new() { "Brown = +12V, white/brown = NEG (each connects on its own).", "Orange = DATA A and green = A1 go together under the device's A terminal.", "White/orange = DATA B and white/green = B1 go together under the B terminal.", "The blue pair is unused." }));
    }
}
