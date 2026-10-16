{
  Parity fixture (issue #38): the atlas builders of
  xejviScriptAdapterHost.pas — wbBuildAtlasFromTexturesList and
  wbBuildAtlasFromAtlasMap — over the fixture textures of
  tests/fixtures/parity/atlas (a case's `data_files` place them in the
  private data folder of both sides). The first call builds an atlas and its
  map file from a texture list; the second rebuilds a second atlas from a
  hand-written map with brightness and the canvas gamma, so the Lanczos
  resizes, the canvas brightness/gamma and the written DDS bytes are
  compared file by file by the harness. The files go into the run folder
  (`ScriptsPath`, mapped to the same `<root>` on both sides), which the
  harness compares: the names are relative with a folder part (`out\...`,
  the folder both sides' runs create; an absolute name would put the run's
  path into the map file, a `textures\` prefix would send the atlas to
  `wbOutputPath`, which a headless run of the two sides does not agree on
  -- the GUI is given `-O:`, the port has no such switch -- and a name with
  no folder part hits an upstream/port difference in the folder creation,
  recorded in the upstream map).
}
unit AtlasProbe;

var
  gOut: string;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

procedure Say(const aTag, aPath: string);
begin
  if FileExists(aPath) then
    Log(aTag, 'exists ' + aPath)
  else
    Log(aTag, 'missing ' + aPath);
end;

function Initialize: Integer;
var
  sl, map: TStringList;
  i: Integer;
begin
  gOut := ScriptsPath;
  Log('probe', 'out=' + gOut);

  sl := TStringList.Create;
  sl.Add('textures\t\one.dds');
  sl.Add('textures\t\two.dds');
  Log('list', 'count=' + IntToStr(sl.Count));
  wbBuildAtlasFromTexturesList(
    sl,
    1024,
    512,
    512,
    512,
    'out\probe_atlas.dds',
    gOut + 'out\probe_map.txt'
  );
  Say('atlas', 'out\probe_atlas.dds');
  Say('map', gOut + 'out\probe_map.txt');
  sl.Free;

  map := TStringList.Create;
  map.Add('textures\t\one.dds' + #9 + '32' + #9 + '32' + #9 + '0' + #9 + '0' + #9 + 'out\probe_gamma.dds' + #9 + '256' + #9 + '256');
  map.Add('textures\t\two.dds' + #9 + '16' + #9 + '16' + #9 + '32' + #9 + '0' + #9 + 'out\probe_gamma.dds' + #9 + '256' + #9 + '256');
  wbBuildAtlasFromAtlasMap(map, 5, 0.9, 1.0, 1.1);
  Say('gamma', 'out\probe_gamma.dds');
  map.Free;

  Result := 0;
end;
end.
