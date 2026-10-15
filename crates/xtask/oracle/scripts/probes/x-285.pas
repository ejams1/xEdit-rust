{ parity probe: x-285 }
unit P_x_285;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  sl.Add(True); AddMessage(sl[1]);
  AddMessage('after');
end;
end.
