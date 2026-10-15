{ parity probe: x-052 }
unit P_x_052;

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
  b := Boolean(5);
  AddMessage('after');
end;
end.
