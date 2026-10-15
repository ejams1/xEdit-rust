{ parity probe: x-306 }
unit P_x_306;

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
  sl.Insert(5, 'x');
  AddMessage('after');
end;
end.
