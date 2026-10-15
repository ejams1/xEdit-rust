{ parity probe: x-020 }
unit P_x_020;

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
  i := Length(5);
  AddMessage('after');
end;
end.
