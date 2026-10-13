import io
import json
import os
import re
import struct
import urllib.request
import urllib.parse
from concurrent.futures import ThreadPoolExecutor, as_completed
from PIL import Image, ImageDraw

ICONS_DIR = os.path.join("assets", "game_icons")
CATALOG_PATH = os.path.join("assets", "games_catalog.json")

# Curated list of popular PC games (Steam AppID, Canonical Title, slug)
CURATED_STEAM_GAMES = [
    (594650, "Hunt: Showdown 1896", "hunt_showdown"),
    (107410, "Arma 3", "arma_3"),
    (1874880, "Arma Reforger", "arma_reforger"),
    (221100, "DayZ", "dayz"),
    (730, "Counter-Strike 2", "cs2"),
    (578080, "PUBG: BATTLEGROUNDS", "pubg"),
    (252490, "Rust", "rust"),
    (359550, "Tom Clancy's Rainbow Six Siege", "r6_siege"),
    (1172470, "Apex Legends", "apex_legends"),
    (393380, "Squad", "squad"),
    (686810, "Hell Let Loose", "hell_let_loose"),
    (2479810, "Gray Zone Warfare", "gray_zone_warfare"),
    (2073620, "Arena Breakout: Infinite", "arena_breakout"),
    (2507950, "Delta Force", "delta_force"),
    (1422450, "Deadlock", "deadlock"),
    (1643320, "S.T.A.L.K.E.R. 2: Heart of Chornobyl", "stalker_2"),
    (513710, "SCUM", "scum"),
    (1144200, "Ready or Not", "ready_or_not"),
    (581320, "Insurgency: Sandstorm", "insurgency_sandstorm"),
    (2073850, "THE FINALS", "the_finals"),
    (1938090, "Call of Duty: Warzone / BO6", "call_of_duty"),
    (2357570, "Overwatch 2", "overwatch_2"),
    (570, "Dota 2", "dota_2"),
    (271590, "Grand Theft Auto V", "gta_v"),
    (1091500, "Cyberpunk 2077", "cyberpunk_2077"),
    (1245620, "ELDEN RING", "elden_ring"),
    (2694490, "Path of Exile 2", "path_of_exile_2"),
    (238960, "Path of Exile", "path_of_exile"),
    (236390, "War Thunder", "war_thunder"),
    (2050650, "Resident Evil 4", "resident_evil_4"),
    (1172620, "Sea of Thieves", "sea_of_thieves"),
    (346110, "ARK: Survival Evolved", "ark_survival"),
    (2399830, "ARK: Survival Ascended", "ark_ascended"),
    (251570, "7 Days to Die", "7_days_to_die"),
    (108600, "Project Zomboid", "project_zomboid"),
    (1326470, "Sons Of The Forest", "sons_of_the_forest"),
    (242760, "The Forest", "the_forest"),
    (892970, "Valheim", "valheim"),
    (1085660, "Destiny 2", "destiny_2"),
    (230410, "Warframe", "warframe"),
    (2139460, "Once Human", "once_human"),
    (2016590, "Dark and Darker", "dark_and_darker"),
    (505460, "Foxhole", "foxhole"),
    (671860, "BattleBit Remastered", "battlebit"),
    (1517290, "Battlefield 2042", "battlefield_2042"),
    (1238810, "Battlefield V", "battlefield_v"),
    (1238840, "Battlefield 1", "battlefield_1"),
    (1238860, "Battlefield 4", "battlefield_4"),
    (552520, "Far Cry 5", "far_cry_5"),
    (220240, "Far Cry 3", "far_cry_3"),
    (381210, "Dead by Daylight", "dead_by_daylight"),
    (739630, "Phasmophobia", "phasmophobia"),
    (1966720, "Lethal Company", "lethal_company"),
    (553850, "HELLDIVERS 2", "helldivers_2"),
    (2183900, "Warhammer 40,000: Space Marine 2", "space_marine_2"),
    (548430, "Deep Rock Galactic", "deep_rock_galactic"),
    (440, "Team Fortress 2", "tf2"),
    (550, "Left 4 Dead 2", "left_4_dead_2"),
    (218620, "PAYDAY 2", "payday_2"),
    (1272080, "PAYDAY 3", "payday_3"),
    (1086940, "Baldur's Gate 3", "baldurs_gate_3"),
    (292030, "The Witcher 3: Wild Hunt", "witcher_3"),
    (1174180, "Red Dead Redemption 2", "rdr2"),
    (413150, "Stardew Valley", "stardew_valley"),
    (105600, "Terraria", "terraria"),
    (4000, "Garry's Mod", "garrys_mod"),
    (244850, "Space Engineers", "space_engineers"),
    (294100, "RimWorld", "rimworld"),
    (427520, "Factorio", "factorio"),
    (526870, "Satisfactory", "satisfactory"),
    (322330, "Don't Starve Together", "dont_starve_together"),
    (264710, "Subnautica", "subnautica"),
    (815370, "Green Hell", "green_hell"),
    (1203620, "Enshrouded", "enshrouded"),
    (1623730, "Palworld", "palworld"),
    (1604030, "V Rising", "v_rising"),
    (440900, "Conan Exiles", "conan_exiles"),
    (1149460, "Icarus", "icarus"),
    (648800, "Raft", "raft"),
    (1366540, "Dyson Sphere Program", "dyson_sphere"),
    (284160, "BeamNG.drive", "beamng"),
    (244210, "Assetto Corsa", "assetto_corsa"),
    (1551360, "Forza Horizon 5", "forza_horizon_5"),
    (1293830, "Forza Horizon 4", "forza_horizon_4"),
    (227300, "Euro Truck Simulator 2", "ets2"),
    (252950, "Rocket League", "rocket_league"),
    (2358720, "Black Myth: Wukong", "black_myth_wukong"),
    (814380, "Sekiro: Shadows Die Twice", "sekiro"),
    (374320, "DARK SOULS III", "dark_souls_3"),
    (377160, "Fallout 4", "fallout_4"),
    (22380, "Fallout: New Vegas", "fallout_nv"),
    (489830, "The Elder Scrolls V: Skyrim", "skyrim"),
    (306130, "The Elder Scrolls Online", "eso"),
    (1599340, "Lost Ark", "lost_ark"),
    (582010, "Monster Hunter: World", "mhw"),
    (1446780, "MONSTER HUNTER RISE", "mh_rise"),
    (594570, "Total War: WARHAMMER II", "tw_warhammer_2"),
    (1142710, "Total War: WARHAMMER III", "tw_warhammer_3"),
    (394360, "Hearts of Iron IV", "hoi4"),
    (289070, "Sid Meier's Civilization VI", "civ6"),
    (813780, "Age of Empires II: Definitive Edition", "aoe2_de"),
    (1466860, "Age of Empires IV", "aoe4"),
    (1222670, "The Sims 4", "sims_4"),
    (1794680, "Vampire Survivors", "vampire_survivors"),
    (2379780, "Balatro", "balatro"),
    (1145360, "Hades", "hades"),
    (1145350, "Hades II", "hades_2"),
    (632360, "Risk of Rain 2", "risk_of_rain_2"),
    (367520, "Hollow Knight", "hollow_knight"),
    (504230, "Celeste", "celeste"),
    (261550, "Mount & Blade II: Bannerlord", "bannerlord"),
    (379430, "Kingdom Come: Deliverance", "kcd"),
    (1771300, "Kingdom Come: Deliverance II", "kcd2"),
    (534380, "Dying Light 2 Stay Human", "dying_light_2"),
    (239140, "Dying Light", "dying_light"),
    (412020, "Metro Exodus", "metro_exodus"),
    (286690, "Metro 2033 Redux", "metro_2033"),
    (287390, "Metro: Last Light Redux", "metro_last_light"),
    (782330, "DOOM Eternal", "doom_eternal"),
    (379720, "DOOM (2016)", "doom_2016"),
    (1237970, "Titanfall 2", "titanfall_2"),
    (601150, "Devil May Cry 5", "dmc5"),
    (1817070, "Marvel's Spider-Man Remastered", "spider_man"),
    (2767030, "Marvel Rivals", "marvel_rivals"),
    (1593500, "God of War", "god_of_war"),
    (883710, "Resident Evil 2", "resident_evil_2"),
    (1196590, "Resident Evil Village", "resident_evil_village"),
    (1063730, "New World: Aeternum", "new_world"),
    (1284210, "Guild Wars 2", "guild_wars_2"),
    (39210, "FINAL FANTASY XIV Online", "ffxiv"),
    (2344520, "Diablo IV", "diablo_4"),
    (1903340, "Clair Obscur: Expedition 33", "expedition_33"),
    (1687950, "Persona 5 Royal", "persona_5"),
]

HEADERS = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"
}


def save_pil_as_rounded_ico(img: Image.Image, ico_path: str):
    """Converts any PIL Image into a crisp 32x32 32-bit RGBA rounded-corner .ico file."""
    size = 32
    img = img.convert("RGBA").resize((size, size), Image.Resampling.LANCZOS)
    # Apply subtle rounded corners mask
    mask = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle((0, 0, size - 1, size - 1), radius=6, fill=255)
    r, g, b, a = img.split()
    img.putalpha(mask)
    img.save(ico_path, format="ICO", sizes=[(32, 32)])


def make_badge_ico(path: str, bg_rgb: tuple, accent_rgb: tuple, glyph: str):
    """Generates a clean 32x32 32-bit BGRA .ico badge for non-Steam standalone games."""
    size = 32
    bgra = bytearray(size * size * 4)
    br, bg, bb = bg_rgb
    ar, ag, ab = accent_rgb

    for y in range(size):
        for x in range(size):
            dx = max(0, abs(x - 15.5) - 10.5)
            dy = max(0, abs(y - 15.5) - 10.5)
            dist = (dx * dx + dy * dy) ** 0.5
            if dist > 4.2:
                continue
            alpha = int(max(0.0, min(1.0, 4.2 - dist)) * 255)
            is_border = dist > 2.6
            is_center = abs(x - 15.5) + abs(y - 15.5) < 6.5 and abs(abs(x - 15.5) - abs(y - 15.5)) < 2.2
            r, g, b = (ar, ag, ab) if (is_border or is_center) else (br, bg, bb)
            inv_y = size - 1 - y
            idx = (inv_y * size + x) * 4
            bgra[idx] = b
            bgra[idx + 1] = g
            bgra[idx + 2] = r
            bgra[idx + 3] = alpha

    and_mask = bytes(((size + 31) // 32) * 4 * size)
    img_size = 40 + len(bgra) + len(and_mask)
    ico = bytearray()
    ico += struct.pack("<HHH", 0, 1, 1)
    ico += struct.pack("<BBBBHHII", size, size, 0, 0, 1, 32, img_size, 22)
    ico += struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, len(bgra), 0, 0, 0, 0)
    ico += bgra
    ico += and_mask
    with open(path, "wb") as f:
        f.write(ico)


def http_get_json(url: str):
    req = urllib.request.Request(url, headers=HEADERS)
    with urllib.request.urlopen(req, timeout=12) as resp:
        if resp.status == 200:
            return json.loads(resp.read().decode("utf-8", errors="ignore"))
    return None


def http_get_bytes(url: str):
    req = urllib.request.Request(url, headers=HEADERS)
    with urllib.request.urlopen(req, timeout=12) as resp:
        if resp.status == 200:
            return resp.read()
    return None


def fetch_single_steam_icon(appid: int, title: str, slug: str):
    ico_filename = f"{slug}_{appid}.ico"
    ico_path = os.path.join(ICONS_DIR, ico_filename)
    if os.path.exists(ico_path) and os.path.getsize(ico_path) > 100:
        return {"title": title, "icon": ico_filename, "appid": appid}

    try:
        # Query SearchApps by game title
        search_term = title.split(":")[0].strip()
        q = urllib.parse.quote(search_term)
        comm_data = http_get_json(f"https://steamcommunity.com/actions/SearchApps/{q}")
        if not comm_data or not isinstance(comm_data, list):
            q2 = urllib.parse.quote(title)
            comm_data = http_get_json(f"https://steamcommunity.com/actions/SearchApps/{q2}")
        if not comm_data or not isinstance(comm_data, list):
            return None

        match = None
        for entry in comm_data:
            if str(entry.get("appid")) == str(appid):
                match = entry
                break
        if not match:
            match = comm_data[0]

        icon_url = match.get("icon", "")
        if not icon_url:
            return None

        # Download the official Steam community icon image (.jpg/.png) and convert to .ico
        raw_img = http_get_bytes(icon_url)
        if raw_img and len(raw_img) > 100:
            pil_img = Image.open(io.BytesIO(raw_img))
            save_pil_as_rounded_ico(pil_img, ico_path)
            return {"title": title, "icon": ico_filename, "appid": appid}
    except Exception:
        pass
    return None


def main():
    os.makedirs(ICONS_DIR, exist_ok=True)
    catalog = []

    print(f"Downloading & converting official Steam icons for {len(CURATED_STEAM_GAMES)} games...")
    with ThreadPoolExecutor(max_workers=6) as pool:
        future_map = {
            pool.submit(fetch_single_steam_icon, appid, title, slug): (appid, title, slug)
            for (appid, title, slug) in CURATED_STEAM_GAMES
        }
        for fut in as_completed(future_map):
            res = fut.result()
            if res:
                catalog.append(res)

    # Add non-Steam standalone PC hits with custom tactical .ico icons
    standalone_games = [
        ("Escape from Tarkov", "escape_from_tarkov.ico", (26, 30, 36), (212, 163, 89)),
        ("VALORANT", "valorant.ico", (24, 30, 42), (255, 70, 85)),
        ("Fortnite", "fortnite.ico", (25, 42, 86), (96, 165, 250)),
        ("Minecraft", "minecraft.ico", (34, 68, 34), (110, 205, 75)),
        ("League of Legends", "league_of_legends.ico", (18, 40, 58), (200, 155, 60)),
        ("World of Tanks", "world_of_tanks.ico", (42, 34, 28), (234, 88, 12)),
        ("World of Warcraft", "world_of_warcraft.ico", (20, 32, 56), (250, 204, 21)),
        ("Enlisted", "enlisted.ico", (36, 44, 36), (163, 230, 53)),
        ("STALCRAFT: X", "stalcraft.ico", (32, 32, 40), (245, 158, 11)),
        ("Caliber", "caliber.ico", (27, 44, 84), (56, 189, 248)),
    ]
    for title, ico_name, bg, fg in standalone_games:
        p = os.path.join(ICONS_DIR, ico_name)
        if not os.path.exists(p):
            make_badge_ico(p, bg, fg, title[:1])
        catalog.append({"title": title, "icon": ico_name, "appid": 0})

    priority = {
        "Hunt: Showdown 1896": 0,
        "Arma 3": 1,
        "Arma Reforger": 2,
        "DayZ": 3,
        "Escape from Tarkov": 4,
        "Counter-Strike 2": 5,
    }
    catalog.sort(key=lambda x: (priority.get(x["title"], 999), x["title"].lower()))

    with open(CATALOG_PATH, "w", encoding="utf-8") as f:
        json.dump(catalog, f, ensure_ascii=False, indent=2)

    print(f"Done! Saved {len(catalog)} game icons in {ICONS_DIR} and manifest in {CATALOG_PATH}")


if __name__ == "__main__":
    main()
