{ parity probe: x-183 }
unit P_x_183;

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
  b := 1 = 1; if b then AddMessage('t');
  AddMessage('after');
end;
end.
