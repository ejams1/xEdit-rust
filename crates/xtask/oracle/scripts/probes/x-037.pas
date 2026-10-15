{ parity probe: x-037 }
unit P_x_037;

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
  foo := 1;
  AddMessage('after');
end;
end.
