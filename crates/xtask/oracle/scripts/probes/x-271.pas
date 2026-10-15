{ parity probe: x-271 }
unit P_x_271;

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
  v := StrToInt(Null);
  AddMessage('after');
end;
end.
