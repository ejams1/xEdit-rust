{ parity probe: x-045 }
unit P_x_045;

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
  i := sl.Count + sl;
  AddMessage('after');
end;
end.
