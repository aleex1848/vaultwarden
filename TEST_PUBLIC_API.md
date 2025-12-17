# Test-Anleitung für Public API Endpunkte

Diese Anleitung zeigt, wie du die neuen `/public/groups*`, `/public/members*` und `/public/collections*` Endpunkte testen kannst.

## Voraussetzungen

1. Vaultwarden muss laufen (entweder lokal oder über Docker)
2. Du musst ein Account haben und Mitglied einer Organisation sein
3. Du musst Admin-Rechte in der Organisation haben, um einen API Key zu erstellen

## Schritt 1: Vaultwarden starten

### Option A: Mit Docker
```bash
docker run --detach --name vaultwarden \
  --volume /vw-data/:/data/ \
  --restart unless-stopped \
  --publish 127.0.0.1:8000:80 \
  vaultwarden/server:latest
```

### Option B: Lokal bauen und starten
```bash
cargo build --release --features sqlite
./target/release/vaultwarden
```

Der Server läuft dann standardmäßig auf `http://localhost:8000`

## Schritt 2: Organization API Key erstellen

Du musst einen Organization API Key über die Web-Vault erstellen:

1. Öffne die Web-Vault: http://localhost:8000 (oder deine Domain)
2. Logge dich ein
3. Gehe zu deiner Organisation → Settings → Organization Info
4. Scrolle runter zu "API Key" und erstelle einen neuen API Key
5. **WICHTIG:** Speichere den API Key sofort, er wird nur einmal angezeigt!
   - Du erhältst ein `client_id` (Format: `organization.<org-uuid>`)
   - Du erhältst ein `client_secret` (der API Key selbst)

Alternativ kannst du den API Key auch über die API erstellen (benötigt normale User-Authentifizierung):

```bash
# Zuerst normal einloggen
curl -X POST "http://localhost:8000/identity/connect/token" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "grant_type=password" \
  -d "username=deine-email@example.com" \
  -d "password=dein-passwort" \
  -d "scope=api"

# Du erhältst ein access_token, das du für den nächsten Schritt brauchst
# Dann API Key erstellen:
curl -X POST "http://localhost:8000/api/organizations/<ORG_ID>/api-key" \
  -H "Authorization: Bearer <ACCESS_TOKEN>" \
  -H "Content-Type: application/json" \
  -d '{"masterPasswordHash": "<PASSWORD_HASH>"}'
```

## Schritt 3: Access Token für Public API erhalten

Um die Public API zu nutzen, musst du zuerst ein Access Token mit dem Organization API Key erhalten:

**WICHTIG:** Du musst auch `device_identifier`, `device_name` und `device_type` angeben:

```bash
export CLIENT_ID="organization.<deine-org-uuid>"
export CLIENT_SECRET="<dein-api-key>"
export DOMAIN="http://localhost:8000"

# Token holen
curl -X POST "${DOMAIN}/identity/connect/token" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "grant_type=client_credentials" \
  -d "scope=api.organization" \
  -d "client_id=${CLIENT_ID}" \
  -d "client_secret=${CLIENT_SECRET}" \
  -d "device_identifier=$(uuidgen 2>/dev/null || echo '00000000-0000-0000-0000-000000000000')" \
  -d "device_name=Public API Client" \
  -d "device_type=14"
```

Die Antwort sieht so aus:
```json
{
  "access_token": "eyJhbGc...",
  "expires_in": 3600,
  "token_type": "Bearer",
  "scope": "api.organization"
}
```

Speichere das `access_token` für die nächsten Schritte:

```bash
export ACCESS_TOKEN="<access_token_aus_antwort>"
```

## Schritt 4: Public API Endpunkte testen

### Gruppen-Endpunkte

#### Alle Gruppen abrufen
```bash
curl -X GET "${DOMAIN}/api/public/groups" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}"
```

#### Alle Gruppen mit Details abrufen
```bash
curl -X GET "${DOMAIN}/api/public/groups/details" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}"
```

#### Eine Gruppe erstellen
```bash
curl -X POST "${DOMAIN}/api/public/groups" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Test Gruppe",
    "accessAll": false,
    "collections": [],
    "users": []
  }'
```

#### Eine Gruppe aktualisieren
```bash
curl -X PUT "${DOMAIN}/api/public/groups/<GROUP_ID>" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Aktualisierte Gruppe",
    "accessAll": false,
    "collections": [],
    "users": ["<MEMBER_ID>"]
  }'
```

#### Eine Gruppe löschen
```bash
curl -X DELETE "${DOMAIN}/api/public/groups/<GROUP_ID>" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}"
```

#### Mitglieder einer Gruppe abrufen
```bash
curl -X GET "${DOMAIN}/api/public/groups/<GROUP_ID>/users" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}"
```

#### Mitglieder einer Gruppe aktualisieren
```bash
curl -X PUT "${DOMAIN}/api/public/groups/<GROUP_ID>/users" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '["<MEMBER_ID_1>", "<MEMBER_ID_2>"]'
```

### Mitglieder-Endpunkte

#### Alle Mitglieder abrufen
```bash
curl -X GET "${DOMAIN}/api/public/members" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}"
```

#### Mitglieder mit Collections und Groups abrufen
```bash
curl -X GET "${DOMAIN}/api/public/members?includeCollections=true&includeGroups=true" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}"
```

#### Ein Mitglied einladen
```bash
curl -X POST "${DOMAIN}/api/public/members/invite" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{
    "emails": ["neue@example.com"],
    "type": "2",
    "accessAll": false,
    "collections": [],
    "groups": []
  }'
```

#### Ein Mitglied bearbeiten
```bash
curl -X PUT "${DOMAIN}/api/public/members/<MEMBER_ID>" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{
    "type": "3",
    "accessAll": false,
    "collections": [{
      "id": "<COLLECTION_ID>",
      "readOnly": false,
      "hidePasswords": false,
      "manage": false
    }],
    "groups": ["<GROUP_ID>"]
  }'
```

#### Ein Mitglied löschen
```bash
curl -X DELETE "${DOMAIN}/api/public/members/<MEMBER_ID>" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}"
```

### Collections-Endpunkte

#### Alle Collections abrufen
```bash
curl -X GET "${DOMAIN}/api/public/collections" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}"
```

#### Eine einzelne Collection abrufen
```bash
curl -X GET "${DOMAIN}/api/public/collections/<COLLECTION_ID>" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}"
```

#### Eine Collection erstellen
```bash
curl -X POST "${DOMAIN}/api/public/collections" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Meine Collection",
    "externalId": "meine-collection",
    "groups": [{
      "id": "<GROUP_ID>",
      "readOnly": false,
      "hidePasswords": false,
      "manage": false
    }],
    "users": [{
      "id": "<MEMBER_ID>",
      "readOnly": false,
      "hidePasswords": false,
      "manage": false
    }]
  }'
```

#### Eine Collection aktualisieren
```bash
curl -X PUT "${DOMAIN}/api/public/collections/<COLLECTION_ID>" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Aktualisierte Collection",
    "externalId": "aktualisierte-collection",
    "groups": [],
    "users": []
  }'
```

#### Eine Collection löschen
```bash
curl -X DELETE "${DOMAIN}/api/public/collections/<COLLECTION_ID>" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}"
```

## Mitglieder-Typen (type)

- `"0"` oder `"Owner"` - Owner
- `"1"` oder `"Admin"` - Admin
- `"2"` oder `"User"` - User (Standard)
- `"3"` oder `"Manager"` - Manager
- `"4"` oder `"Custom"` - Custom (wird zu Manager konvertiert)

## Vollständiges Test-Script

Hier ist ein vollständiges Test-Script:

```bash
#!/bin/bash

# Konfiguration
DOMAIN="http://localhost:8000"
CLIENT_ID="organization.<deine-org-uuid>"
CLIENT_SECRET="<dein-api-key>"

# Token holen
echo "Hole Access Token..."
TOKEN_RESPONSE=$(curl -s -X POST "${DOMAIN}/identity/connect/token" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "grant_type=client_credentials" \
  -d "scope=api.organization" \
  -d "client_id=${CLIENT_ID}" \
  -d "client_secret=${CLIENT_SECRET}" \
  -d "device_identifier=$(uuidgen 2>/dev/null || echo '00000000-0000-0000-0000-000000000000')" \
  -d "device_name=Public API Client" \
  -d "device_type=14")

ACCESS_TOKEN=$(echo $TOKEN_RESPONSE | jq -r '.access_token')

if [ "$ACCESS_TOKEN" == "null" ] || [ -z "$ACCESS_TOKEN" ]; then
  echo "Fehler beim Abrufen des Tokens:"
  echo $TOKEN_RESPONSE
  exit 1
fi

echo "Token erhalten: ${ACCESS_TOKEN:0:20}..."

# Gruppen abrufen
echo -e "\n=== Alle Gruppen abrufen ==="
curl -s -X GET "${DOMAIN}/api/public/groups" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}" | jq '.'

# Mitglieder abrufen
echo -e "\n=== Alle Mitglieder abrufen ==="
curl -s -X GET "${DOMAIN}/api/public/members" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}" | jq '.'

# Collections abrufen
echo -e "\n=== Alle Collections abrufen ==="
curl -s -X GET "${DOMAIN}/api/public/collections" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}" | jq '.'

echo -e "\nTest abgeschlossen!"
```

## Fehlerbehebung

### "No access token provided"
- Stelle sicher, dass du den Authorization Header mit `Bearer <token>` sendest

### "Invalid claim" oder "Token expired"
- Hole ein neues Token (Token ist 1 Stunde gültig)

### "device_identifier cannot be blank"
- Stelle sicher, dass du `device_identifier`, `device_name` und `device_type` beim Token-Abruf mitgibst

### "Group support is disabled"
- Gruppen müssen in der Vaultwarden Konfiguration aktiviert sein

### "Organization not found"
- Prüfe, dass die Organization ID im Token korrekt ist

### Collections haben verschlüsselte Namen
- Collection-Namen werden in Bitwarden clientseitig verschlüsselt gespeichert
- Collections, die über die Public API erstellt werden, haben Klartext-Namen
- Für bestehende Collections: Nutze `externalId` wenn möglich, oder erstelle neue Collections über die Public API

## Hinweise

- Alle Tokens sind 1 Stunde gültig (`expires_in: 3600`)
- Du kannst den Token jederzeit neu abrufen
- Die Public API benötigt keine User-Authentifizierung, nur den Organization API Key
- Alle Endpunkte erwarten JSON-Format
- Collections erstellt über die Public API haben Klartext-Namen (nicht verschlüsselt)



