{
  Parity probe (issue #25): wbGetUVRangeTexturesList over a LOD mesh the
  case places in the data folder (the flat LOD mesh of the release's Edit
  Scripts), a mesh nobody has, a billboard texture and a name that is no
  mesh, into an empty and into a sorted texture list.
}
unit HostUVRange;

function B(aValue: Boolean): string;
begin
  if aValue then
    Result := 'True'
  else
    Result := 'False';
end;

procedure Show(aTag: string; sl: TStringList);
var
  i: Integer;
begin
  AddMessage(aTag + ' count ' + IntToStr(sl.Count));
  for i := 0 to Pred(sl.Count) do
    AddMessage(aTag + '   ' + sl[i] + ' object ' + B(Assigned(sl.Objects[i])));
end;

function Initialize: Integer;
var
  meshes, textures: TStringList;
begin
  Result := 0;
  meshes := TStringList.Create;
  textures := TStringList.Create;
  try
    meshes.Add('meshes\lod\host25\flat.nif');
    meshes.Add('lod\host25\flat.nif');
    meshes.Add('meshes\lod\host25\missing.nif');
    meshes.Add('textures\lod\host25billboard.dds');
    meshes.Add('meshes\lod\host25\notes.txt');
    wbGetUVRangeTexturesList(meshes, textures, 1.0);
    Show('unsorted', textures);
    textures.Clear;
    textures.Sorted := True;
    textures.Add('textures\zz\first.dds');
    wbGetUVRangeTexturesList(meshes, textures, 0.5);
    Show('sorted', textures);
  finally
    meshes.Free;
    textures.Free;
  end;
end;

end.
