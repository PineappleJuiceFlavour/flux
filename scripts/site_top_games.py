"""Write docs/top-games.json for fluxlauncher.com: the five most played games on Steam right now,
with the details the site's Home-screen demo shows (name, art, genre, developer, release date, short
description, today's peak players).

Runs in the Pages workflow (daily and on every push). If Steam can't be reached, it leaves the file
alone and the page falls back to the list built into it."""
import json
import sys
import urllib.request
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "docs" / "top-games.json"
CDN = "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/{appid}/{file}"
SOFTWARE = {"Animation & Modeling", "Audio Production", "Design & Illustration", "Education", "Game Development",
            "Photo Editing", "Software Training", "Utilities", "Video Production", "Web Publishing"}
UA = {"User-Agent": "fluxlauncher.com top-games (+https://github.com/PineappleJuiceFlavour/flux)"}


def get_json(url: str):
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=20) as r:
        return json.load(r)


def exists(url: str) -> bool:
    try:
        req = urllib.request.Request(url, headers=UA, method="HEAD")
        with urllib.request.urlopen(req, timeout=15) as r:
            return r.status == 200
    except Exception:
        return False


def art(appid: int, file: str, fallback: str | None) -> str | None:
    url = CDN.format(appid=appid, file=file)
    return url if exists(url) else fallback


def main() -> int:
    try:
        ranks = get_json("https://api.steampowered.com/ISteamChartsService/GetMostPlayedGames/v1/")["response"]["ranks"]
    except Exception as e:
        print("Steam charts unreachable, keeping the old list:", e)
        return 0
    games = []
    for rank in ranks:
        if len(games) == 5:
            break
        appid = rank["appid"]
        try:
            data = get_json(f"https://store.steampowered.com/api/appdetails?appids={appid}&cc=us&l=english")[str(appid)]
        except Exception as e:
            print("skip", appid, e)
            continue
        if not data.get("success") or data["data"].get("type") != "game":
            continue  # tools and soundtracks sometimes chart too
        d = data["data"]
        genres = {g["description"] for g in d.get("genres", [])}
        if genres & SOFTWARE:
            continue  # apps sold as "games" (Wallpaper Engine and friends)
        header = d.get("header_image")
        games.append({
            "appid": appid,
            "name": d["name"],
            "description": d.get("short_description", ""),
            "genres": [g["description"] for g in d.get("genres", [])][:3],
            "developer": (d.get("developers") or ["—"])[0],
            "publisher": (d.get("publishers") or ["—"])[0],
            "released": d.get("release_date", {}).get("date", "—"),
            "free": d.get("is_free", False),
            "peak": rank.get("peak_in_game", 0),
            "cover": art(appid, "library_600x900.jpg", header),
            "hero": art(appid, "library_hero.jpg", header),
            "logo": art(appid, "logo.png", None),
        })
        print("ok", appid, d["name"])
    if len(games) < 5:
        print("only found", len(games), "games, keeping the old list")
        return 0
    OUT.write_text(json.dumps({"source": "Steam most played", "games": games}, indent=1), encoding="utf-8")
    print("wrote", OUT)
    return 0


if __name__ == "__main__":
    sys.exit(main())
