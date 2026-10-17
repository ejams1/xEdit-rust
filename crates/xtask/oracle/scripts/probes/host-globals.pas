{
  Parity probe (issue #25): the zero-argument globals of
  xejviScriptAdapterHost.pas -- the four load options, the settings file
  name, the settings object and the record definition map.
}
unit HostGlobals;

function B(aValue: Boolean): string;
begin
  if aValue then
    Result := 'True'
  else
    Result := 'False';
end;

procedure Show(aName: string; aValue: Variant);
begin
  AddMessage(aName + ' = ' + VarToStr(aValue) + ' (VarType ' + IntToStr(VarType(aValue)) + ')');
end;

function Initialize: Integer;
var
  sl: TStringList;
  ini: TMemIniFile;
  i: Integer;
begin
  Result := 0;
  Show('wbLoadBSAs', wbLoadBSAs);
  Show('wbSimpleRecords', wbSimpleRecords);
  Show('wbDecodeTextureHashes', wbDecodeTextureHashes);
  Show('wbTrackAllEditorID', wbTrackAllEditorID);
  Show('wbSettingsFileName', wbSettingsFileName);
  AddMessage('wbSettingsFileName name: ' + ExtractFileName(wbSettingsFileName));

  ini := wbSettings;
  AddMessage('wbSettings assigned: ' + B(Assigned(ini)));
  AddMessage('wbSettings same object: ' + B(wbSettings = ini));
  AddMessage('wbSettings ReadString: ' + ini.ReadString('Probe25', 'Missing', 'default'));
  ini.WriteString('Probe25', 'Key', 'written');
  AddMessage('wbSettings read back: ' + wbSettings.ReadString('Probe25', 'Key', 'default'));
  AddMessage('wbSettings SectionExists: ' + B(wbSettings.SectionExists('Probe25')));
  ini.EraseSection('Probe25');
  AddMessage('wbSettings erased: ' + B(wbSettings.SectionExists('Probe25')));

  sl := wbRecordDefMap;
  AddMessage('wbRecordDefMap assigned: ' + B(Assigned(sl)));
  AddMessage('wbRecordDefMap same object: ' + B(wbRecordDefMap = sl));
  AddMessage('wbRecordDefMap Count: ' + IntToStr(sl.Count));
  AddMessage('wbRecordDefMap Sorted: ' + B(sl.Sorted));
  AddMessage('wbRecordDefMap IndexOf WEAP: ' + IntToStr(sl.IndexOf('WEAP')));
  AddMessage('wbRecordDefMap IndexOf weap: ' + IntToStr(sl.IndexOf('weap')));
  AddMessage('wbRecordDefMap IndexOf XXXX: ' + IntToStr(sl.IndexOf('XXXX')));
  AddMessage('wbRecordDefMap object assigned: ' + B(Assigned(sl.Objects[0])));
  for i := 0 to Pred(sl.Count) do
    AddMessage('  ' + IntToStr(i) + ' ' + sl[i]);
end;

end.
