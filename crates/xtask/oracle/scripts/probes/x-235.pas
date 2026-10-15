{ parity probe: x-235 }
unit P_x_235;

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
  v := FileByIndex(0); s := v; AddMessage(s);
  AddMessage('after');
end;
end.
