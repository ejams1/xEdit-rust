{
  Parity probe (issue #25): the LOD generators of xejviScriptAdapterHost.pas
  over the HostWorld worldspace of the host-25 fixture (its lodsettings
  file, the static's LOD mesh placed in the data folder). The run folder
  (wbScriptsPath) holds no LODGenx64.exe, so the objects pass writes its
  export and fails to start the tool; the trees pass looks for billboards
  nobody made. GenerateLODTES4 does nothing outside Oblivion.
}
unit HostLOD;

procedure Run(aTag: string; aCase: Integer; aWorld: IInterface);
begin
  try
    case aCase of
      1: GenerateLODTES5Objects(aWorld);
      2: GenerateLODTES5Trees(aWorld);
      3: GenerateLODTES4(aWorld);
    end;
    AddMessage(aTag + ' returned');
  except
    on E: Exception do
      AddMessage(aTag + ' raised ' + E.Message);
  end;
end;

function Initialize: Integer;
var
  w: IInterface;
begin
  Result := 0;
  w := RecordByHexFormID('00000B00');
  if not Assigned(w) then begin
    AddMessage('the host-25 fixture is not loaded');
    Exit;
  end;
  // Full reads first (the quick init of a record named first loses its
  // subrecords upstream, issue #64).
  ElementByPath(w, 'EDID');
  ElementByPath(RecordByHexFormID('00000901'), 'EDID');
  ElementByPath(RecordByHexFormID('00000902'), 'EDID');
  ElementByPath(RecordByHexFormID('00000B01'), 'XCLC');
  ElementByPath(RecordByHexFormID('00000B02'), 'EDID');
  ElementByPath(RecordByHexFormID('00000B03'), 'EDID');
  AddMessage('worldspace ' + Name(w));
  Run('GenerateLODTES4', 3, w);
  Run('GenerateLODTES5Trees', 2, w);
  Run('GenerateLODTES5Objects', 1, w);
end;

end.
