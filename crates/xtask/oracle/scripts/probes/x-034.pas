{ parity probe: x-034 }
unit P_x_034;

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
  i := 1 shl 40;
  AddMessage('after');
end;
end.
