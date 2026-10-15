{ parity probe: x-019 }
unit P_x_019;

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
  i := High(5);
  AddMessage('after');
end;
end.
