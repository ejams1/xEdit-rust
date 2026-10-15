{ parity probe: x-006 }
unit P_x_006;

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
  i := 2147483647 + 1;
  AddMessage('after');
end;
end.
