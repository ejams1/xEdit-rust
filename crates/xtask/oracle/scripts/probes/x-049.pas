{ parity probe: x-049 }
unit P_x_049;

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
  i := Integer(1.5);
  AddMessage('after');
end;
end.
